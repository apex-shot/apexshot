use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
#[cfg(not(target_os = "windows"))]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use crate::capture::editor::image_transform::StaticImageTransform;
#[cfg(not(target_os = "windows"))]
use crate::recording::editor::model::{VideoEditState, ZoomCameraSettings, ZoomClip};

const KEY_ID: &str = "studio-v1";
const PRODUCT: &str = "apexshot-studio";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
/// Schema 1 licences carry no device binding. Those issued before this
/// instant (2026-11-23T00:00:00Z) stay valid everywhere; the service issues
/// only device-bound schema 2 licences from then on, so a later schema 1
/// token is refused.
const UNBOUND_SCHEMA_ISSUED_BEFORE: i64 = 1_795_392_000;
#[cfg(not(target_os = "windows"))]
const IDENTITY_EPSILON: f64 = 1e-7;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
std::thread_local! {
    static TEST_ENTITLEMENT_PATH_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
    static TEST_DEFAULT_ENTITLEMENT_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StudioCapabilities {
    pub motion_export: bool,
    pub static_presentation: bool,
    pub advanced_zoom: bool,
    pub image_upscale: bool,
    /// Decorative still-image presentation: frame styles and scene shadows.
    pub studio_style: bool,
    /// On-screen keystroke captions baked into a recording.
    pub keystroke_captions: bool,
    /// The camera person cut out of the webcam and set on the screen.
    pub camera_cutout: bool,
}

impl StudioCapabilities {
    pub fn requires_studio(self) -> bool {
        self.motion_export
            || self.static_presentation
            || self.advanced_zoom
            || self.image_upscale
            || self.studio_style
            || self.keystroke_captions
            || self.camera_cutout
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioStatus {
    ConfigurationUnavailable,
    NotActivated,
    InvalidEntitlement,
    /// A schema 2 licence issued to a different machine, or a schema 1 licence
    /// issued after device binding became mandatory.
    DeviceMismatch,
    Active {
        updates_until: i64,
    },
    UpdateAccessEnded {
        updates_until: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioUpdateCoverage {
    Included,
    NotIncluded,
    Unknown,
    NotApplicable,
}

impl StudioStatus {
    pub fn message(self) -> String {
        match self {
            Self::ConfigurationUnavailable => "Studio isn't available in this build configuration. Free capture and editing remain available.".into(),
            Self::NotActivated => "Studio isn't activated. Free capture, editing, recording, and clean exports continue to work offline.".into(),
            Self::InvalidEntitlement => "The saved Studio entitlement couldn't be verified. Import a valid entitlement to activate this build.".into(),
            Self::DeviceMismatch => "This licence is bound to another device. Open the Studio settings on this machine, copy its Device ID, and claim the licence for it at apexshot.org/studio.".into(),
            Self::Active { updates_until } => format!(
                "Studio is active for this build. Included updates run through {}.",
                format_timestamp(updates_until)
            ),
            Self::UpdateAccessEnded { updates_until } => format!(
                "This build is outside the included update period. Covered Studio builds remain usable offline; updates were included through {}.",
                format_timestamp(updates_until)
            ),
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum StudioEntitlementError {
    #[error("Studio activation is unavailable in this build configuration")]
    ConfigurationUnavailable,
    #[error("Studio isn't activated on this device")]
    NotActivated,
    #[error("The Studio entitlement is invalid or could not be verified")]
    InvalidEntitlement,
    #[error("This Studio licence is bound to a different device")]
    DeviceMismatch,
    #[error("This licence needs to be claimed for this device. Copy the Device ID from Studio settings and claim the licence at apexshot.org/studio.")]
    UnboundLicenceRetired,
    #[error("This build is outside the entitlement's included update period")]
    UpdateAccessEnded,
    #[error("Studio entitlement storage failed: {0}")]
    Storage(String),
}

#[derive(Clone)]
struct BuildConfiguration {
    verifying_key: VerifyingKey,
    release_timestamp: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementEnvelope {
    key_id: String,
    payload: String,
    signature: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct EntitlementPayload {
    schema_version: u8,
    license_id: String,
    product: String,
    issued_at: i64,
    updates_until: i64,
    // Schema 2 only. Optional so schema 1 tokens still deserialize; `deny_unknown_fields`
    // would otherwise reject every licence issued before device binding existed.
    #[serde(default)]
    install_id: Option<String>,
    #[serde(default)]
    licensee: Option<String>,
    #[serde(default)]
    devices_max: Option<u8>,
}

pub fn current_status() -> StudioStatus {
    let Ok(configuration) = build_configuration() else {
        return StudioStatus::ConfigurationUnavailable;
    };
    let path = match entitlement_path() {
        Ok(path) => path,
        Err(_) => return StudioStatus::InvalidEntitlement,
    };
    match read_entitlement(&path, &configuration) {
        Ok(Some(entitlement))
            if build_is_covered(configuration.release_timestamp, entitlement.updates_until) =>
        {
            StudioStatus::Active {
                updates_until: entitlement.updates_until,
            }
        }
        Ok(Some(entitlement)) => StudioStatus::UpdateAccessEnded {
            updates_until: entitlement.updates_until,
        },
        Ok(None) => StudioStatus::NotActivated,
        Err(StudioEntitlementError::DeviceMismatch)
        | Err(StudioEntitlementError::UnboundLicenceRetired) => StudioStatus::DeviceMismatch,
        Err(_) => StudioStatus::InvalidEntitlement,
    }
}

/// Whether an active licence already covers this build, so an export that needs
/// Studio would not stop for activation. Panels use it to skip the "needs
/// activation" preview copy instead of demanding a step the user has taken.
pub fn studio_unlocked() -> bool {
    matches!(current_status(), StudioStatus::Active { .. })
}

pub fn activate_token(token: &str) -> Result<(), StudioEntitlementError> {
    let configuration = build_configuration()?;
    let path = entitlement_path()?;
    activate_token_at_path(&path, token, &configuration)?;
    // The Windows capture shell reads the licence state from its settings
    // mirror, so publish the activation instead of waiting for Settings.
    #[cfg(all(target_os = "windows", not(test)))]
    if let Err(error) = crate::settings::actions::sync_shell_settings_on_start() {
        eprintln!("[studio] could not refresh the shell settings: {error}");
    }
    Ok(())
}

/// Activates the licence carried by an `apexshot://studio/activate` link.
///
/// The purchase page hands a device-bound licence back through this link, with
/// the signed token envelope as unpadded base64url in its `token` parameter.
/// The token is verified exactly like a pasted one, so a link from any page can
/// only install a licence signed by the official key and bound to this device.
pub fn activate_from_url(url: &str) -> Result<(), StudioEntitlementError> {
    let token = decode_activation_url(url)?;
    activate_token(&token)
}

fn decode_activation_url(url: &str) -> Result<String, StudioEntitlementError> {
    let query = url
        .strip_prefix("apexshot://studio/activate?")
        .or_else(|| url.strip_prefix("apexshot://studio/activate/?"))
        .ok_or(StudioEntitlementError::InvalidEntitlement)?;
    if query.is_empty() || query.contains('#') {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }

    // The link carries exactly one parameter; anything else is malformed.
    if query.contains('&') {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let token = query
        .strip_prefix("token=")
        .ok_or(StudioEntitlementError::InvalidEntitlement)?;

    let decoded = percent_decode(token)?;
    if decoded.len() > MAX_TOKEN_BYTES {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let token =
        String::from_utf8(decoded).map_err(|_| StudioEntitlementError::InvalidEntitlement)?;
    let bytes = decode_base64url(token.trim_end_matches('='))?;
    if bytes.len() > MAX_TOKEN_BYTES {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    String::from_utf8(bytes).map_err(|_| StudioEntitlementError::InvalidEntitlement)
}

fn percent_decode(value: &str) -> Result<Vec<u8>, StudioEntitlementError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes
                .get(index + 1)
                .copied()
                .and_then(hex_value)
                .ok_or(StudioEntitlementError::InvalidEntitlement)?;
            let low = bytes
                .get(index + 2)
                .copied()
                .and_then(hex_value)
                .ok_or(StudioEntitlementError::InvalidEntitlement)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The entitlement text saved on this device, exactly as it was accepted.
///
/// Settings offers it so a license can be carried to another machine without a
/// trip back to the purchase page. It is deliberately unverified: the caller
/// wants the stored bytes, and `current_status` already answers whether this
/// build can use them.
pub fn stored_token() -> Option<String> {
    let path = entitlement_path().ok()?;
    let token = fs::read_to_string(path).ok()?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn activate_token_at_path(
    path: &Path,
    token: &str,
    configuration: &BuildConfiguration,
) -> Result<(), StudioEntitlementError> {
    let token = token.trim();
    let bytes = token.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_TOKEN_BYTES {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let entitlement = verify_token(token, &configuration.verifying_key)?;
    let local = crate::usage_telemetry::ensure_install_id();
    if bound_to_other_device(&entitlement, local.as_deref()) {
        // Saving it would leave the app permanently mismatched with no way to
        // tell the user why, so this is refused outright.
        return Err(StudioEntitlementError::DeviceMismatch);
    }
    write_token_atomically(path, token)
}

/// How this build may treat one output's Studio capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StudioAccess {
    /// The output needs no Studio capability and is always free.
    NotNeeded,
    /// A verified licence covers this build.
    Licensed,
    /// No licence covers this build, but free trial exports remain.
    Trial { remaining: u32 },
    /// The output needs Studio and neither a licence nor the trial allows it.
    Blocked(StudioEntitlementError),
}

impl StudioAccess {
    pub fn is_trial(&self) -> bool {
        matches!(self, Self::Trial { .. })
    }
}

/// Decide what an output with `capabilities` may do right now.
///
/// The free trial applies to every refusal that activation could fix: a
/// missing, invalid, mismatched, or uncovered licence. A build that cannot
/// verify licences, or cannot read its storage, never falls back to the
/// trial, because the user could not end the trial by buying Studio anyway.
pub fn studio_access(capabilities: StudioCapabilities) -> StudioAccess {
    if !capabilities.requires_studio() {
        return StudioAccess::NotNeeded;
    }
    match licence_allows_studio() {
        Ok(()) => StudioAccess::Licensed,
        Err(error) => {
            let remaining = match &error {
                StudioEntitlementError::ConfigurationUnavailable
                | StudioEntitlementError::Storage(_) => 0,
                _ => crate::studio_trial::trial_remaining(),
            };
            access_for_licence_result(Err(error), remaining)
        }
    }
}

/// The licence check `studio_access` starts from: a valid, covering token.
fn licence_allows_studio() -> Result<(), StudioEntitlementError> {
    let configuration = build_configuration()?;
    let path = entitlement_path()?;
    let entitlement =
        read_entitlement(&path, &configuration)?.ok_or(StudioEntitlementError::NotActivated)?;
    if !build_is_covered(configuration.release_timestamp, entitlement.updates_until) {
        return Err(StudioEntitlementError::UpdateAccessEnded);
    }
    Ok(())
}

fn access_for_licence_result(
    licence: Result<(), StudioEntitlementError>,
    remaining: u32,
) -> StudioAccess {
    match licence {
        Ok(()) => StudioAccess::Licensed,
        Err(error) => {
            let trial_applies = remaining > 0
                && !matches!(
                    &error,
                    StudioEntitlementError::ConfigurationUnavailable
                        | StudioEntitlementError::Storage(_)
                );
            if trial_applies {
                StudioAccess::Trial { remaining }
            } else {
                StudioAccess::Blocked(error)
            }
        }
    }
}

/// Consume one free export when `access` started as a trial.
///
/// Every layer of one export shares the same check, so only the output's own
/// completion calls this. A licensed or free output leaves the counter alone.
pub fn finish_trial_export(access: StudioAccess) -> Option<u32> {
    access
        .is_trial()
        .then(crate::studio_trial::record_trial_export)
}

/// Whether the output may proceed. A trial allows it without consuming
/// anything; only the finished output calls [`finish_trial_export`].
pub fn ensure_export_allowed(
    capabilities: StudioCapabilities,
) -> Result<(), StudioEntitlementError> {
    match studio_access(capabilities) {
        StudioAccess::Blocked(error) => Err(error),
        _ => Ok(()),
    }
}

pub fn update_coverage_for_release(
    release_timestamp: Option<i64>,
    release_key_fingerprint: Option<&str>,
) -> StudioUpdateCoverage {
    let Ok(configuration) = build_configuration() else {
        return StudioUpdateCoverage::NotApplicable;
    };
    let Ok(path) = entitlement_path() else {
        return StudioUpdateCoverage::Unknown;
    };
    match read_entitlement(&path, &configuration) {
        Ok(Some(entitlement)) => update_coverage(
            release_timestamp,
            release_key_fingerprint,
            Some(&entitlement),
            &verifying_key_fingerprint(&configuration.verifying_key),
        ),
        Ok(None) => StudioUpdateCoverage::NotApplicable,
        Err(_) => StudioUpdateCoverage::Unknown,
    }
}

fn update_coverage(
    release_timestamp: Option<i64>,
    release_key_fingerprint: Option<&str>,
    entitlement: Option<&EntitlementPayload>,
    current_key_fingerprint: &str,
) -> StudioUpdateCoverage {
    let Some(entitlement) = entitlement else {
        return StudioUpdateCoverage::NotApplicable;
    };
    let Some(release_timestamp) = release_timestamp else {
        return StudioUpdateCoverage::Unknown;
    };
    if release_timestamp <= 0 || release_key_fingerprint != Some(current_key_fingerprint) {
        return StudioUpdateCoverage::Unknown;
    }
    if build_is_covered(release_timestamp, entitlement.updates_until) {
        StudioUpdateCoverage::Included
    } else {
        StudioUpdateCoverage::NotIncluded
    }
}

fn verifying_key_fingerprint(key: &VerifyingKey) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(key.as_bytes());
    let mut fingerprint = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String succeeds");
    }
    fingerprint
}

pub fn capabilities_for_static_image(transform: StaticImageTransform) -> StudioCapabilities {
    StudioCapabilities {
        static_presentation: !transform.sanitized().is_identity(),
        ..StudioCapabilities::default()
    }
}

/// The still-image export's capabilities: the frozen pose plus the
/// decorative appearance. The caller computes `style` because only the
/// document knows its appearance.
pub fn capabilities_for_image_style(
    transform: StaticImageTransform,
    style: bool,
) -> StudioCapabilities {
    StudioCapabilities {
        studio_style: style,
        ..capabilities_for_static_image(transform)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capabilities_for_video(state: &VideoEditState) -> StudioCapabilities {
    if state.zoom_hidden {
        return StudioCapabilities::default();
    }

    let zoom_clips = state
        .zoom_clips
        .iter()
        .filter(|clip| {
            !clip.hidden
                && clip.start.is_finite()
                && clip.end.is_finite()
                && clip.end > clip.start
                && clip.scale.is_finite()
                && clip.scale > 1.0 + IDENTITY_EPSILON
        })
        .collect::<Vec<_>>();
    let visible_zooms = zoom_clips
        .iter()
        .copied()
        .filter(|clip| clip_intersects_visible_video(state, clip))
        .collect::<Vec<_>>();
    let camera = state.zoom_camera.clamped();
    let defaults = ZoomCameraSettings::default();
    let has_early_window = !state.zoom_classic
        && camera.start_early
        && has_early_window(state, &zoom_clips, camera.lead_in_seconds());
    let has_animated_zoom = visible_zooms.iter().any(|clip| !clip.instant) || has_early_window;
    let nondefault_camera = has_animated_zoom
        && ((camera.motion_blur - defaults.motion_blur).abs() > IDENTITY_EPSILON
            || (!state.zoom_classic
                && ((camera.speed - defaults.speed).abs() > IDENTITY_EPSILON
                    || (camera.smoothness - defaults.smoothness).abs() > IDENTITY_EPSILON
                    || has_early_window)));

    StudioCapabilities {
        advanced_zoom: visible_zooms.iter().any(|clip| clip.instant) || nondefault_camera,
        ..StudioCapabilities::default()
    }
}

pub fn capabilities_for_motion_export() -> StudioCapabilities {
    StudioCapabilities {
        motion_export: true,
        ..StudioCapabilities::default()
    }
}

#[cfg(not(target_os = "windows"))]
fn clip_intersects_visible_video(state: &VideoEditState, clip: &ZoomClip) -> bool {
    if clip.hidden || !clip.start.is_finite() || !clip.end.is_finite() || clip.end <= clip.start {
        return false;
    }
    let boundaries = state.segment_boundaries();
    state.segment_order.iter().any(|&index| {
        if !state.segments_kept.get(index).copied().unwrap_or(true) {
            return false;
        }
        let Some(&(source_start, source_end)) = boundaries.get(index) else {
            return false;
        };
        if source_end <= source_start {
            return false;
        }
        let start = state.segment_start(index);
        let freeze_tail = if state.freeze_applies_to_segment(index) {
            state.freeze_tail_seconds()
        } else {
            0.0
        };
        let end = start + state.segment_timeline_duration(index) + freeze_tail;
        end > start && clip.start < end && clip.end > start
    })
}

#[cfg(not(target_os = "windows"))]
fn has_early_window(state: &VideoEditState, clips: &[&ZoomClip], lead_in_seconds: f64) -> bool {
    let boundaries = state.segment_boundaries();
    clips.iter().any(|clip| {
        if clip.instant {
            return false;
        }
        let lead_start = (clip.start - lead_in_seconds).max(0.0);
        state.segment_order.iter().any(|&index| {
            if !state.segments_kept.get(index).copied().unwrap_or(true) {
                return false;
            }
            let Some(&(source_start, source_end)) = boundaries.get(index) else {
                return false;
            };
            if source_end <= source_start {
                return false;
            }
            let start = state.segment_start(index);
            let freeze_tail = if state.freeze_applies_to_segment(index) {
                state.freeze_tail_seconds()
            } else {
                0.0
            };
            let end = start + state.segment_timeline_duration(index) + freeze_tail;
            start < clip.start && end > lead_start
        })
    })
}

#[cfg(not(test))]
fn build_configuration() -> Result<BuildConfiguration, StudioEntitlementError> {
    parse_build_configuration(
        option_env!("APEXSHOT_STUDIO_VERIFY_KEY"),
        option_env!("APEXSHOT_STUDIO_RELEASE_TIMESTAMP"),
    )
}

#[cfg(test)]
fn build_configuration() -> Result<BuildConfiguration, StudioEntitlementError> {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    Ok(BuildConfiguration {
        verifying_key: signing_key.verifying_key(),
        release_timestamp: 200,
    })
}

fn parse_build_configuration(
    encoded_key: Option<&str>,
    timestamp: Option<&str>,
) -> Result<BuildConfiguration, StudioEntitlementError> {
    let encoded_key = encoded_key.ok_or(StudioEntitlementError::ConfigurationUnavailable)?;
    let timestamp = timestamp.ok_or(StudioEntitlementError::ConfigurationUnavailable)?;
    let key_bytes = decode_base64url(encoded_key)
        .map_err(|_| StudioEntitlementError::ConfigurationUnavailable)?;
    let key_bytes: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| StudioEntitlementError::ConfigurationUnavailable)?;
    let verifying_key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| StudioEntitlementError::ConfigurationUnavailable)?;
    if timestamp.is_empty()
        || timestamp.starts_with('0')
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(StudioEntitlementError::ConfigurationUnavailable);
    }
    let release_timestamp = timestamp
        .parse::<i64>()
        .ok()
        .filter(|timestamp| *timestamp > 0)
        .ok_or(StudioEntitlementError::ConfigurationUnavailable)?;
    Ok(BuildConfiguration {
        verifying_key,
        release_timestamp,
    })
}

fn verify_token(
    token: &str,
    verifying_key: &VerifyingKey,
) -> Result<EntitlementPayload, StudioEntitlementError> {
    let envelope: EntitlementEnvelope =
        serde_json::from_str(token).map_err(|_| StudioEntitlementError::InvalidEntitlement)?;
    if envelope.key_id != KEY_ID {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let payload = decode_base64url(&envelope.payload)?;
    let signature = decode_base64url(&envelope.signature)?;
    if payload.len() > MAX_TOKEN_BYTES {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let signature = Signature::from_slice(&signature)
        .map_err(|_| StudioEntitlementError::InvalidEntitlement)?;
    verifying_key
        .verify_strict(&payload, &signature)
        .map_err(|_| StudioEntitlementError::InvalidEntitlement)?;

    let entitlement: EntitlementPayload =
        serde_json::from_slice(&payload).map_err(|_| StudioEntitlementError::InvalidEntitlement)?;
    if entitlement.product != PRODUCT
        || !is_uuid(&entitlement.license_id)
        || entitlement.issued_at <= 0
        || entitlement.updates_until < entitlement.issued_at
    {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    match entitlement.schema_version {
        // Schema 1 predates device binding; only licences issued before the
        // device-binding cutoff stay valid everywhere.
        1 => {
            if entitlement.install_id.is_some()
                || entitlement.licensee.is_some()
                || entitlement.devices_max.is_some()
            {
                return Err(StudioEntitlementError::InvalidEntitlement);
            }
            if entitlement.issued_at >= UNBOUND_SCHEMA_ISSUED_BEFORE {
                return Err(StudioEntitlementError::UnboundLicenceRetired);
            }
        }
        2 => {
            let install_id = entitlement
                .install_id
                .as_deref()
                .ok_or(StudioEntitlementError::InvalidEntitlement)?;
            if !is_uuid(install_id)
                || entitlement
                    .licensee
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                || !matches!(entitlement.devices_max, Some(1..=16))
            {
                return Err(StudioEntitlementError::InvalidEntitlement);
            }
        }
        _ => return Err(StudioEntitlementError::InvalidEntitlement),
    }
    Ok(entitlement)
}

/// This machine's Studio device ID, created on first use.
///
/// The website needs it before a licence exists, and the Studio settings page
/// offers it for copying, so it has to be readable without an entitlement.
pub fn device_id() -> Option<String> {
    crate::usage_telemetry::ensure_install_id()
}

/// The official Studio page, carrying this machine's Device ID so a purchase
/// can be bound to it without the customer copying anything.
///
/// Falls back to the plain page when no id is available; the customer can then
/// still copy the Device ID from Settings and claim the licence by hand.
pub fn studio_page_url() -> String {
    let Some(device_id) = device_id() else {
        return "https://apexshot.org/studio".to_string();
    };
    if !is_uuid(&device_id) {
        return "https://apexshot.org/studio".to_string();
    }
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    format!("https://apexshot.org/studio?device={device_id}&from=app&platform={platform}")
}

/// One line a customer can paste into a support request: the licence state and
/// whether this build can verify licences at all. Never includes the token.
pub fn studio_status_line() -> String {
    match current_status() {
        StudioStatus::ConfigurationUnavailable => {
            "studio: unavailable (this build cannot verify licences)".to_string()
        }
        StudioStatus::NotActivated => "studio: not activated".to_string(),
        StudioStatus::InvalidEntitlement => {
            "studio: saved licence could not be verified".to_string()
        }
        StudioStatus::DeviceMismatch => {
            "studio: licence is bound to a different device".to_string()
        }
        StudioStatus::Active { updates_until } => {
            format!("studio: active (updates through {updates_until})")
        }
        StudioStatus::UpdateAccessEnded { updates_until } => {
            format!("studio: build newer than licence (updates through {updates_until})")
        }
    }
}

/// Whether a verified entitlement was issued for a machine other than this one.
///
/// Schema 1 licences carry no binding and always pass; only schema 2 binds.
fn bound_to_other_device(entitlement: &EntitlementPayload, local_install_id: Option<&str>) -> bool {
    if entitlement.schema_version != 2 {
        return false;
    }
    let Some(bound) = entitlement.install_id.as_deref() else {
        return false;
    };
    match local_install_id {
        Some(local) => !local.eq_ignore_ascii_case(bound),
        // A build that cannot read its own install id cannot prove the binding
        // either way; refuse rather than silently granting a shared licence.
        None => true,
    }
}

fn decode_base64url(encoded: &str) -> Result<Vec<u8>, StudioEntitlementError> {
    if encoded.contains('=')
        || encoded.is_empty()
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| StudioEntitlementError::InvalidEntitlement)?;
    if URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    Ok(decoded)
}

fn is_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

#[cfg(not(test))]
fn entitlement_path() -> Result<PathBuf, StudioEntitlementError> {
    dirs::config_dir()
        .map(|directory| directory.join("apexshot/studio/entitlement.json"))
        .ok_or_else(|| {
            StudioEntitlementError::Storage("configuration directory is unavailable".into())
        })
}

#[cfg(test)]
fn entitlement_path() -> Result<PathBuf, StudioEntitlementError> {
    Ok(TEST_ENTITLEMENT_PATH_OVERRIDE
        .with(|path| path.borrow().clone())
        .unwrap_or_else(|| {
            TEST_DEFAULT_ENTITLEMENT_PATH.with(|path| {
                path.borrow_mut()
                    .get_or_insert_with(new_test_entitlement_path)
                    .clone()
            })
        }))
}

fn read_entitlement(
    path: &Path,
    configuration: &BuildConfiguration,
) -> Result<Option<EntitlementPayload>, StudioEntitlementError> {
    let token = match fs::read_to_string(path) {
        Ok(token) => token,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StudioEntitlementError::Storage(error.to_string())),
    };
    if token.len() > MAX_TOKEN_BYTES {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    let entitlement = verify_token(token.trim(), &configuration.verifying_key)?;
    let local = crate::usage_telemetry::ensure_install_id();
    if bound_to_other_device(&entitlement, local.as_deref()) {
        return Err(StudioEntitlementError::DeviceMismatch);
    }
    Ok(Some(entitlement))
}

fn write_token_atomically(path: &Path, token: &str) -> Result<(), StudioEntitlementError> {
    let parent = path.parent().ok_or_else(|| {
        StudioEntitlementError::Storage("configuration path has no parent".into())
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| StudioEntitlementError::Storage(error.to_string()))?;
    set_private_directory_permissions(parent)?;

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".studio-entitlement-{}-{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| StudioEntitlementError::Storage(error.to_string()))?;
        file.write_all(token.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| StudioEntitlementError::Storage(error.to_string()))?;
        fs::rename(&temporary, path)
            .map_err(|error| StudioEntitlementError::Storage(error.to_string()))?;
        #[cfg(not(target_os = "windows"))]
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| StudioEntitlementError::Storage(error.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), StudioEntitlementError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| StudioEntitlementError::Storage(error.to_string()))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), StudioEntitlementError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut OpenOptions) {}

fn format_timestamp(timestamp: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| timestamp.to_string())
}

fn build_is_covered(release_timestamp: i64, updates_until: i64) -> bool {
    release_timestamp <= updates_until
}

#[cfg(test)]
pub(crate) struct TestEntitlementGuard {
    previous_path: Option<PathBuf>,
    cleanup_directory: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for TestEntitlementGuard {
    fn drop(&mut self) {
        TEST_ENTITLEMENT_PATH_OVERRIDE.with(|path| {
            *path.borrow_mut() = self.previous_path.take();
        });
        if let Some(directory) = self.cleanup_directory.take() {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

#[cfg(test)]
fn new_test_entitlement_path() -> PathBuf {
    std::env::temp_dir()
        .join(format!(
            "apexshot-studio-entitlement-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
        .join("entitlement.json")
}

#[cfg(test)]
fn test_entitlement_path_scope(path: PathBuf) -> TestEntitlementGuard {
    let previous_path = TEST_ENTITLEMENT_PATH_OVERRIDE.with(|current| current.replace(Some(path)));
    TestEntitlementGuard {
        previous_path,
        cleanup_directory: None,
    }
}

#[cfg(test)]
pub(crate) fn test_studio_entitlement() -> TestEntitlementGuard {
    let path = new_test_entitlement_path();
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
    let configuration = build_configuration().expect("test build config");
    let payload = serde_json::json!({
        "schema_version": 1,
        "license_id": "550e8400-e29b-41d4-a716-446655440000",
        "product": PRODUCT,
        "issued_at": 100,
        "updates_until": 3_000_000_000_i64,
    })
    .to_string();
    let token = signed_token_for_test(&signing_key, payload.as_bytes());
    activate_token_at_path(&path, &token, &configuration).expect("save test entitlement");
    let mut guard = test_entitlement_path_scope(path.clone());
    guard.cleanup_directory = path.parent().map(Path::to_path_buf);
    guard
}

#[cfg(test)]
fn signed_token_for_test(signing_key: &ed25519_dalek::SigningKey, payload: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ed25519_dalek::Signer;
    let signature = signing_key.sign(payload);
    serde_json::json!({
        "key_id": KEY_ID,
        "payload": URL_SAFE_NO_PAD.encode(payload),
        "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    })
    .to_string()
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    fn signed_token(device: Option<&str>, updates_until: i64) -> String {
        let mut payload = serde_json::json!({
            "schema_version": 1,
            "license_id": "550e8400-e29b-41d4-a716-446655440000",
            "product": PRODUCT,
            "issued_at": 100,
            "updates_until": updates_until,
        });
        if let Some(device) = device {
            payload["schema_version"] = 2.into();
            payload["install_id"] = device.into();
            payload["licensee"] = "user@example.invalid".into();
            payload["devices_max"] = 3.into();
        }
        signed_token_for_test(
            &ed25519_dalek::SigningKey::from_bytes(&[7; 32]),
            payload.to_string().as_bytes(),
        )
    }

    #[test]
    fn activation_checks_signature_device_and_build_coverage() {
        let path = new_test_entitlement_path();
        let mut scope = test_entitlement_path_scope(path.clone());
        scope.cleanup_directory = path.parent().map(Path::to_path_buf);
        // This test is about the licence states themselves, not the trial.
        let _trial = crate::studio_trial::test_trial_used_up();
        let capabilities = StudioCapabilities {
            static_presentation: true,
            ..Default::default()
        };
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::NotActivated)
        );
        assert_eq!(
            activate_token("{}"),
            Err(StudioEntitlementError::InvalidEntitlement)
        );
        let other = signed_token(Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"), 300);
        assert_eq!(
            activate_token(&other),
            Err(StudioEntitlementError::DeviceMismatch)
        );
        let expired = signed_token(None, 150);
        activate_token(&expired).unwrap();
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::UpdateAccessEnded)
        );
        let valid = signed_token(device_id().as_deref(), 300);
        activate_token(&valid).unwrap();
        assert_eq!(stored_token(), Some(valid));
        assert!(ensure_export_allowed(capabilities).is_ok());
        assert!(matches!(current_status(), StudioStatus::Active { .. }));
    }

    #[test]
    fn build_configuration_requires_a_valid_public_key_and_timestamp() {
        let signing = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let encoded = URL_SAFE_NO_PAD.encode(signing.verifying_key().as_bytes());
        assert!(parse_build_configuration(Some(&encoded), Some("200")).is_ok());
        assert!(parse_build_configuration(None, Some("200")).is_err());
        assert!(parse_build_configuration(Some(&encoded), Some("0")).is_err());
        assert!(parse_build_configuration(Some("invalid"), Some("200")).is_err());
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    #[test]
    fn neural_upscale_requires_studio_without_changing_free_image_exports() {
        let transform = StaticImageTransform::default();
        assert!(!capabilities_for_static_image(transform).requires_studio());
        let capabilities = StudioCapabilities {
            image_upscale: true,
            ..capabilities_for_static_image(transform)
        };
        assert!(capabilities.image_upscale);
        assert!(capabilities.requires_studio());
        let path = scratch_path();
        let _path = test_entitlement_path_scope(path);
        // The free trial would otherwise let this output through.
        let _trial = crate::studio_trial::test_trial_used_up();
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::NotActivated)
        );
    }

    #[test]
    fn a_decorative_style_needs_studio_but_a_clean_still_stays_free() {
        let clean = StaticImageTransform::default();
        assert!(!capabilities_for_image_style(clean, false).requires_studio());
        let styled = capabilities_for_image_style(clean, true);
        assert!(styled.studio_style);
        assert!(styled.requires_studio());
    }

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ed25519_dalek::{Signer, SigningKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn synthetic_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn signed_token(signing_key: &SigningKey, payload: &[u8]) -> String {
        let signature = signing_key.sign(payload);
        serde_json::json!({
            "key_id": KEY_ID,
            "payload": URL_SAFE_NO_PAD.encode(payload),
            "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        })
        .to_string()
    }

    fn valid_payload(updates_until: i64) -> Vec<u8> {
        serde_json::json!({
            "schema_version": 1,
            "license_id": "550e8400-e29b-41d4-a716-446655440000",
            "product": PRODUCT,
            "issued_at": 100,
            "updates_until": updates_until,
        })
        .to_string()
        .into_bytes()
    }

    fn video_state() -> VideoEditState {
        VideoEditState::new(crate::recording::editor::model::VideoMetadata {
            path: PathBuf::from("/nonexistent/studio-entitlement-fixture.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 0,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    fn scratch_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "apexshot-studio-entitlement-{}-{stamp}.json",
                std::process::id()
            ))
            .join("entitlement.json")
    }

    #[test]
    fn strict_signature_verifies_the_exact_original_payload_bytes() {
        let signing_key = synthetic_signing_key();
        let key = signing_key.verifying_key();
        let payload = br#"{ "schema_version": 1, "license_id": "550e8400-e29b-41d4-a716-446655440000", "product": "apexshot-studio", "issued_at": 100, "updates_until": 300 }"#;
        let token = signed_token(&signing_key, payload);

        assert_eq!(verify_token(&token, &key).unwrap().updates_until, 300);

        let envelope: EntitlementEnvelope = serde_json::from_str(&token).unwrap();
        let mut changed_payload = URL_SAFE_NO_PAD.decode(envelope.payload).unwrap();
        changed_payload.insert(1, b' ');
        let tampered = serde_json::json!({
            "key_id": KEY_ID,
            "payload": URL_SAFE_NO_PAD.encode(changed_payload),
            "signature": envelope.signature,
        })
        .to_string();
        assert_eq!(
            verify_token(&tampered, &key),
            Err(StudioEntitlementError::InvalidEntitlement)
        );
    }

    #[test]
    fn cloud_issuer_golden_token_verifies_against_its_exact_payload_bytes() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/studio-entitlement-v1.json"))
                .unwrap();
        let public_key = fixture["public_key"].as_str().unwrap();
        let release_timestamp = fixture["release_timestamp"].as_i64().unwrap();
        let release_timestamp_text = release_timestamp.to_string();
        let configuration =
            parse_build_configuration(Some(public_key), Some(&release_timestamp_text)).unwrap();
        assert_eq!(
            verifying_key_fingerprint(&configuration.verifying_key),
            "10ba682c8ad13513971e8b56881aab8bd702bb807796eca81932c735a94d6e6d"
        );
        let envelope = &fixture["envelope"];
        let payload = decode_base64url(envelope["payload"].as_str().unwrap()).unwrap();

        assert_eq!(
            payload,
            fixture["payload_json"].as_str().unwrap().as_bytes()
        );
        let entitlement =
            verify_token(&envelope.to_string(), &configuration.verifying_key).unwrap();
        assert_eq!(
            entitlement.license_id,
            "123e4567-e89b-12d3-a456-426614174000"
        );
        assert_eq!(entitlement.issued_at, 1_792_022_400);
        assert_eq!(entitlement.updates_until, 1_823_558_400);
        assert!(build_is_covered(
            configuration.release_timestamp,
            entitlement.updates_until
        ));
    }

    #[test]
    fn token_fields_and_signature_are_all_required_and_validated() {
        let signing_key = synthetic_signing_key();
        let key = signing_key.verifying_key();
        let valid = valid_payload(300);
        let token = signed_token(&signing_key, &valid);
        assert!(verify_token(&token, &key).is_ok());
        let envelope: EntitlementEnvelope = serde_json::from_str(&token).unwrap();
        for malformed in [
            String::new(),
            "not-json".to_string(),
            serde_json::json!({
                "key_id": KEY_ID,
                "payload": envelope.payload,
                "signature": envelope.signature,
                "extra": true,
            })
            .to_string(),
            serde_json::json!({
                "key_id": KEY_ID,
                "payload": "AA=",
                "signature": "AA",
            })
            .to_string(),
        ] {
            assert_eq!(
                verify_token(&malformed, &key),
                Err(StudioEntitlementError::InvalidEntitlement)
            );
        }

        for payload in [
            serde_json::json!({
                "schema_version": 2,
                "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT,
                "issued_at": 100,
                "updates_until": 300,
            })
            .to_string()
            .into_bytes(),
            serde_json::json!({
                "schema_version": 1,
                "license_id": "not-a-uuid",
                "product": PRODUCT,
                "issued_at": 100,
                "updates_until": 300,
            })
            .to_string()
            .into_bytes(),
            serde_json::json!({
                "schema_version": 1,
                "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": "apexshot-cloud",
                "issued_at": 100,
                "updates_until": 300,
            })
            .to_string()
            .into_bytes(),
            serde_json::json!({
                "schema_version": 1,
                "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT,
                "issued_at": 301,
                "updates_until": 300,
            })
            .to_string()
            .into_bytes(),
            serde_json::json!({
                "schema_version": 1,
                "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT,
                "issued_at": 100,
                "updates_until": 300,
                "extra": true,
            })
            .to_string()
            .into_bytes(),
        ] {
            assert_eq!(
                verify_token(&signed_token(&signing_key, &payload), &key),
                Err(StudioEntitlementError::InvalidEntitlement)
            );
        }
    }

    #[test]
    fn update_eligibility_uses_the_immutable_build_marker_not_wall_clock() {
        let signing_key = synthetic_signing_key();
        let key = signing_key.verifying_key();
        let token = signed_token(&signing_key, &valid_payload(300));
        let entitlement = verify_token(&token, &key).unwrap();

        assert!(build_is_covered(250, entitlement.updates_until));
        assert!(build_is_covered(300, entitlement.updates_until));
        assert!(!build_is_covered(301, entitlement.updates_until));
    }

    #[test]
    fn update_notice_coverage_uses_only_the_target_release_marker() {
        let entitlement: EntitlementPayload = serde_json::from_slice(&valid_payload(300)).unwrap();
        let current_key_fingerprint =
            verifying_key_fingerprint(&synthetic_signing_key().verifying_key());

        assert_eq!(
            update_coverage(
                Some(299),
                Some(&current_key_fingerprint),
                Some(&entitlement),
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::Included
        );
        assert_eq!(
            update_coverage(
                Some(300),
                Some(&current_key_fingerprint),
                Some(&entitlement),
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::Included
        );
        assert_eq!(
            update_coverage(
                Some(301),
                Some(&current_key_fingerprint),
                Some(&entitlement),
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::NotIncluded
        );
        assert_eq!(
            update_coverage(
                None,
                Some(&current_key_fingerprint),
                Some(&entitlement),
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::Unknown
        );
        assert_eq!(
            update_coverage(
                Some(301),
                Some("b".repeat(64).as_str()),
                Some(&entitlement),
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::Unknown
        );
        assert_eq!(
            update_coverage(
                Some(301),
                Some(&current_key_fingerprint),
                None,
                &current_key_fingerprint
            ),
            StudioUpdateCoverage::NotApplicable
        );
    }

    #[test]
    fn export_guard_distinguishes_missing_invalid_and_uncovered_tokens() {
        let path = scratch_path();
        let _path = test_entitlement_path_scope(path.clone());
        // No free trial here: this test is about licence failures.
        let _trial = crate::studio_trial::test_trial_used_up();
        let capabilities = StudioCapabilities {
            advanced_zoom: true,
            ..StudioCapabilities::default()
        };
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::NotActivated)
        );

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not-json").unwrap();
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::InvalidEntitlement)
        );

        let signing_key = synthetic_signing_key();
        write_token_atomically(&path, &signed_token(&signing_key, &valid_payload(199))).unwrap();
        assert_eq!(
            ensure_export_allowed(capabilities),
            Err(StudioEntitlementError::UpdateAccessEnded)
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn access_decision_distinguishes_free_licensed_trial_and_blocked() {
        let path = scratch_path();
        let _path = test_entitlement_path_scope(path.clone());
        let _trial = crate::studio_trial::test_trial_path_scope();

        assert_eq!(
            studio_access(StudioCapabilities::default()),
            StudioAccess::NotNeeded
        );

        let needed = StudioCapabilities {
            advanced_zoom: true,
            ..StudioCapabilities::default()
        };
        assert_eq!(studio_access(needed), StudioAccess::Trial { remaining: 3 });

        let signing_key = synthetic_signing_key();
        write_token_atomically(&path, &signed_token(&signing_key, &valid_payload(300))).unwrap();
        assert_eq!(studio_access(needed), StudioAccess::Licensed);
        assert_eq!(
            crate::studio_trial::trial_remaining(),
            crate::studio_trial::STUDIO_FREE_EXPORTS,
            "a licence never consumes the trial"
        );

        fs::remove_file(&path).unwrap();
        for _ in 0..crate::studio_trial::STUDIO_FREE_EXPORTS {
            crate::studio_trial::record_trial_export();
        }
        assert_eq!(
            studio_access(needed),
            StudioAccess::Blocked(StudioEntitlementError::NotActivated)
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn configuration_and_storage_failures_never_become_a_trial() {
        assert_eq!(
            access_for_licence_result(
                Err(StudioEntitlementError::ConfigurationUnavailable),
                crate::studio_trial::STUDIO_FREE_EXPORTS
            ),
            StudioAccess::Blocked(StudioEntitlementError::ConfigurationUnavailable)
        );
        assert_eq!(
            access_for_licence_result(
                Err(StudioEntitlementError::Storage("disk full".into())),
                crate::studio_trial::STUDIO_FREE_EXPORTS
            ),
            StudioAccess::Blocked(StudioEntitlementError::Storage("disk full".into()))
        );
        assert_eq!(
            access_for_licence_result(
                Err(StudioEntitlementError::NotActivated),
                crate::studio_trial::STUDIO_FREE_EXPORTS
            ),
            StudioAccess::Trial { remaining: 3 }
        );
        assert_eq!(access_for_licence_result(Ok(()), 0), StudioAccess::Licensed);
    }

    #[test]
    fn a_trial_export_passes_without_consuming_a_free_export() {
        let path = scratch_path();
        let _path = test_entitlement_path_scope(path);
        let _trial = crate::studio_trial::test_trial_path_scope();
        let capabilities = StudioCapabilities {
            advanced_zoom: true,
            ..StudioCapabilities::default()
        };

        assert_eq!(
            crate::studio_trial::trial_remaining(),
            crate::studio_trial::STUDIO_FREE_EXPORTS
        );
        assert_eq!(ensure_export_allowed(capabilities), Ok(()));
        assert_eq!(
            crate::studio_trial::trial_remaining(),
            crate::studio_trial::STUDIO_FREE_EXPORTS
        );

        assert_eq!(finish_trial_export(studio_access(capabilities)), Some(2));
        assert_eq!(crate::studio_trial::trial_remaining(), 2);
        assert_eq!(
            finish_trial_export(StudioAccess::Licensed),
            None,
            "a licensed output never consumes the trial"
        );
        assert_eq!(crate::studio_trial::trial_remaining(), 2);
    }

    #[test]
    fn activated_file_is_reverified_after_a_fresh_entitlement_path_scope() {
        let path = scratch_path();
        let signing_key = synthetic_signing_key();
        let token = signed_token(&signing_key, &valid_payload(300));
        let capabilities = StudioCapabilities {
            advanced_zoom: true,
            ..StudioCapabilities::default()
        };

        {
            let _path = test_entitlement_path_scope(path.clone());
            activate_token(&token).unwrap();
        }
        {
            let _fresh_path = test_entitlement_path_scope(path.clone());
            assert_eq!(ensure_export_allowed(capabilities), Ok(()));
            assert_eq!(
                current_status(),
                StudioStatus::Active { updates_until: 300 }
            );
        }

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_activation_keeps_the_previous_valid_token() {
        let signing_key = synthetic_signing_key();
        let key = signing_key.verifying_key();
        let previous = signed_token(&signing_key, &valid_payload(300));
        let path = scratch_path();
        let configuration = BuildConfiguration {
            verifying_key: key,
            release_timestamp: 250,
        };
        write_token_atomically(&path, &previous).unwrap();
        assert_eq!(
            read_entitlement(&path, &configuration)
                .unwrap()
                .unwrap()
                .updates_until,
            300
        );

        assert_eq!(
            activate_token_at_path(&path, "not-json", &configuration),
            Err(StudioEntitlementError::InvalidEntitlement)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), previous);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn invalid_token_diagnostics_do_not_echo_token_contents() {
        let token = "test-only-sensitive-token-content";
        let path = scratch_path();
        let error = activate_token_at_path(
            &path,
            token,
            &BuildConfiguration {
                verifying_key: synthetic_signing_key().verifying_key(),
                release_timestamp: 200,
            },
        )
        .unwrap_err();

        assert_eq!(error, StudioEntitlementError::InvalidEntitlement);
        assert!(!error.to_string().contains(token));
        assert!(!path.exists());
    }

    #[test]
    fn free_defaults_classic_animation_and_legacy_zero_ease_remain_free() {
        assert_eq!(ensure_export_allowed(StudioCapabilities::default()), Ok(()));
        let transform = StaticImageTransform::default();
        assert!(!capabilities_for_static_image(transform).requires_studio());

        let mut state = video_state();
        state.zoom_classic = true;
        state.zoom_clips.push(ZoomClip {
            ease_ms: 0,
            ..ZoomClip::default()
        });
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_camera.speed = 2.0;
        state.zoom_camera.smoothness = 0.9;
        state.zoom_camera.start_early = true;
        assert!(!capabilities_for_video(&state).requires_studio());

        let mut perspective = transform;
        perspective.perspective = 0.4;
        assert!(!capabilities_for_static_image(perspective).requires_studio());
        perspective.yaw = 5.0;
        assert!(capabilities_for_static_image(perspective).static_presentation);
    }

    #[test]
    fn unused_zoom_card_pose_fields_do_not_gate_export() {
        let mut state = video_state();
        state.zoom_clips.push(ZoomClip {
            rotation_y: 8.0,
            ..ZoomClip::default()
        });
        assert!(!capabilities_for_video(&state).requires_studio());
    }

    #[test]
    fn zoom_visibility_and_exported_segments_determine_studio_requirements() {
        let mut state = video_state();
        state.zoom_clips.push(ZoomClip {
            instant: true,
            hidden: true,
            ..ZoomClip::default()
        });
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_clips[0].hidden = false;
        state.zoom_hidden = true;
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_hidden = false;
        state.video_hidden = true;
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.video_hidden = false;
        state.segments_kept[0] = false;
        assert!(!capabilities_for_video(&state).requires_studio());
    }

    #[test]
    fn freeze_tail_and_its_early_window_are_part_of_the_exported_zoom() {
        let mut state = video_state();
        state.extend_last_segment(1.0);
        state.zoom_clips.push(ZoomClip {
            start: 10.5,
            end: 10.9,
            instant: true,
            ..ZoomClip::default()
        });
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_clips[0].start = 10.8;
        state.zoom_clips[0].end = 11.0;
        state.zoom_clips[0].instant = false;
        state.zoom_camera.start_early = true;
        assert!(capabilities_for_video(&state).advanced_zoom);
    }

    #[test]
    fn camera_controls_and_instant_are_paid_only_on_active_applicable_zooms() {
        let mut state = video_state();
        state.zoom_camera.speed = 2.0;
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_camera.speed = 1.0;
        state.zoom_clips.push(ZoomClip::default());
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_clips[0].instant = true;
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_clips[0].instant = false;
        state.zoom_clips[0].start = 1.0;
        state.zoom_clips[0].end = 3.0;
        state.zoom_camera.speed = 2.0;
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_camera.speed = 1.0;
        state.zoom_camera.smoothness = 0.5;
        state.zoom_camera.start_early = true;
        assert!(capabilities_for_video(&state).advanced_zoom);
    }

    #[test]
    fn motion_blur_is_paid_only_when_an_animated_zoom_uses_it() {
        let mut state = video_state();
        state.zoom_camera.motion_blur = 0.4;
        assert!(!capabilities_for_video(&state).requires_studio());

        state.zoom_clips.push(ZoomClip::default());
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_classic = true;
        assert!(capabilities_for_video(&state).advanced_zoom);
    }

    #[test]
    fn early_camera_window_covers_short_leads_and_lead_only_output_overlap() {
        let mut state = video_state();
        state.zoom_clips.push(ZoomClip {
            start: 0.2,
            end: 1.0,
            ..ZoomClip::default()
        });
        assert!(!capabilities_for_video(&state).requires_studio());
        state.zoom_camera.start_early = true;
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_clips[0].start = 10.2;
        state.zoom_clips[0].end = 11.0;
        assert!(capabilities_for_video(&state).advanced_zoom);

        state.zoom_clips[0].start = 10.36;
        state.zoom_clips[0].end = 11.0;
        assert!(!capabilities_for_video(&state).requires_studio());
    }

    #[test]
    fn missing_or_malformed_official_build_configuration_is_unavailable() {
        let key = URL_SAFE_NO_PAD.encode(synthetic_signing_key().verifying_key().to_bytes());
        assert!(matches!(
            parse_build_configuration(None, Some("200")),
            Err(StudioEntitlementError::ConfigurationUnavailable)
        ));
        assert!(matches!(
            parse_build_configuration(Some(&key), None),
            Err(StudioEntitlementError::ConfigurationUnavailable)
        ));
        for timestamp in ["", "0", "0200", "200x", "9223372036854775808"] {
            assert!(matches!(
                parse_build_configuration(Some(&key), Some(timestamp)),
                Err(StudioEntitlementError::ConfigurationUnavailable)
            ));
        }
    }

    #[test]
    fn motion_export_is_studio_but_project_preview_is_not_a_capability_check() {
        assert!(capabilities_for_motion_export().motion_export);
        assert!(!StudioCapabilities::default().requires_studio());
    }

    fn verify_synthetic(
        payload: serde_json::Value,
    ) -> Result<EntitlementPayload, StudioEntitlementError> {
        let token = signed_token_for_test(&synthetic_signing_key(), payload.to_string().as_bytes());
        verify_token(&token, &synthetic_signing_key().verifying_key())
    }

    fn schema1_payload() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "license_id": "550e8400-e29b-41d4-a716-446655440000",
            "product": PRODUCT,
            "issued_at": 100,
            "updates_until": 3_000_000_000_i64,
        })
    }

    fn schema2_payload(install_id: &str) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 2,
            "license_id": "550e8400-e29b-41d4-a716-446655440000",
            "product": PRODUCT,
            "issued_at": 100,
            "updates_until": 3_000_000_000_i64,
            "install_id": install_id,
            "licensee": "buyer@example.com",
            "devices_max": 3,
        })
    }

    #[test]
    fn schema1_licences_still_verify_after_device_binding_lands() {
        // Regression: deny_unknown_fields means a non-optional v2 field would
        // lock every pre-existing customer out on upgrade.
        let entitlement = verify_synthetic(schema1_payload()).expect("v1 must still verify");
        assert_eq!(entitlement.schema_version, 1);
        assert!(!bound_to_other_device(&entitlement, Some("any-device")));
        assert!(!bound_to_other_device(&entitlement, None));
    }

    #[test]
    fn schema1_licences_issued_before_the_device_binding_cutoff_stay_valid() {
        let mut payload = schema1_payload();
        payload["issued_at"] = serde_json::json!(UNBOUND_SCHEMA_ISSUED_BEFORE - 1);
        let entitlement =
            verify_synthetic(payload).expect("licences issued before the cutoff stay valid");
        assert_eq!(entitlement.schema_version, 1);
    }

    #[test]
    fn schema1_licences_issued_at_or_after_the_cutoff_are_retired() {
        for issued_at in [
            UNBOUND_SCHEMA_ISSUED_BEFORE,
            UNBOUND_SCHEMA_ISSUED_BEFORE + 86_400 * 30,
        ] {
            let mut payload = schema1_payload();
            payload["issued_at"] = serde_json::json!(issued_at);
            assert_eq!(
                verify_synthetic(payload),
                Err(StudioEntitlementError::UnboundLicenceRetired)
            );
        }
    }

    #[test]
    fn a_stored_retired_unbound_licence_reports_a_device_mismatch() {
        let path = scratch_path();
        let _scope = test_entitlement_path_scope(path.clone());
        let mut payload = schema1_payload();
        payload["issued_at"] = serde_json::json!(UNBOUND_SCHEMA_ISSUED_BEFORE);
        let token = signed_token_for_test(&synthetic_signing_key(), payload.to_string().as_bytes());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, token).unwrap();
        assert_eq!(current_status(), StudioStatus::DeviceMismatch);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_retired_unbound_licence_is_never_saved() {
        let path = scratch_path();
        let configuration = build_configuration().expect("test build config");
        let mut payload = schema1_payload();
        payload["issued_at"] = serde_json::json!(UNBOUND_SCHEMA_ISSUED_BEFORE);
        let token = signed_token_for_test(&synthetic_signing_key(), payload.to_string().as_bytes());
        let result = activate_token_at_path(&path, &token, &configuration);
        assert_eq!(result, Err(StudioEntitlementError::UnboundLicenceRetired));
        assert!(!path.exists());
    }

    #[test]
    fn schema2_licences_issued_after_the_cutoff_still_verify_for_their_device() {
        let device = "8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b";
        let mut payload = schema2_payload(device);
        payload["issued_at"] = serde_json::json!(UNBOUND_SCHEMA_ISSUED_BEFORE + 86_400);
        let entitlement = verify_synthetic(payload).expect("schema 2 verifies after the cutoff");
        assert_eq!(entitlement.schema_version, 2);
        assert!(!bound_to_other_device(&entitlement, Some(device)));
    }

    #[test]
    fn schema2_licence_matches_only_the_device_it_was_issued_to() {
        let device = "8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b";
        let entitlement = verify_synthetic(schema2_payload(device)).expect("v2 verifies");
        assert!(!bound_to_other_device(&entitlement, Some(device)));
        assert!(!bound_to_other_device(
            &entitlement,
            Some(&device.to_ascii_uppercase())
        ));
        assert!(bound_to_other_device(
            &entitlement,
            Some("11111111-2222-3333-4444-555555555555")
        ));
        // A machine that cannot read its own id must not be granted the licence.
        assert!(bound_to_other_device(&entitlement, None));
    }

    #[test]
    fn schema2_requires_a_well_formed_binding() {
        for broken in [
            serde_json::json!({
                "schema_version": 2, "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT, "issued_at": 100, "updates_until": 3_000_000_000_i64,
                "licensee": "buyer@example.com", "devices_max": 3,
            }),
            schema2_payload("not-a-uuid"),
            serde_json::json!({
                "schema_version": 2, "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT, "issued_at": 100, "updates_until": 3_000_000_000_i64,
                "install_id": "8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b",
                "licensee": "   ", "devices_max": 3,
            }),
            serde_json::json!({
                "schema_version": 2, "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT, "issued_at": 100, "updates_until": 3_000_000_000_i64,
                "install_id": "8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b",
                "licensee": "buyer@example.com", "devices_max": 0,
            }),
            serde_json::json!({
                "schema_version": 2, "license_id": "550e8400-e29b-41d4-a716-446655440000",
                "product": PRODUCT, "issued_at": 100, "updates_until": 3_000_000_000_i64,
                "install_id": "8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b",
                "licensee": "buyer@example.com", "devices_max": 17,
            }),
        ] {
            assert_eq!(
                verify_synthetic(broken),
                Err(StudioEntitlementError::InvalidEntitlement)
            );
        }
    }

    #[test]
    fn schema1_rejects_binding_fields_and_unknown_versions() {
        let mut with_binding = schema1_payload();
        with_binding["install_id"] = serde_json::json!("8f14e45f-ceea-467f-a1b2-3c5d6e7f8a9b");
        assert_eq!(
            verify_synthetic(with_binding),
            Err(StudioEntitlementError::InvalidEntitlement)
        );

        let mut unknown = schema1_payload();
        unknown["schema_version"] = serde_json::json!(3);
        assert_eq!(
            verify_synthetic(unknown),
            Err(StudioEntitlementError::InvalidEntitlement)
        );
    }

    #[test]
    fn a_token_for_another_device_is_never_saved() {
        let path = scratch_path();
        let configuration = build_configuration().expect("test build config");
        let other = "11111111-2222-3333-4444-555555555555";
        let token = signed_token_for_test(
            &synthetic_signing_key(),
            schema2_payload(other).to_string().as_bytes(),
        );
        let result = activate_token_at_path(&path, &token, &configuration);
        // Either the local id differs (mismatch) or cannot be read (also refused);
        // both must leave nothing on disk.
        assert_eq!(result, Err(StudioEntitlementError::DeviceMismatch));
        assert!(!path.exists());
    }

    #[test]
    fn an_activation_link_saves_a_device_bound_licence() {
        let device = device_id().expect("test device id");
        let token = signed_token_for_test(
            &synthetic_signing_key(),
            schema2_payload(&device).to_string().as_bytes(),
        );
        let encoded = URL_SAFE_NO_PAD.encode(token.as_bytes());
        let mut padded = encoded.clone();
        while !padded.len().is_multiple_of(4) {
            padded.push('=');
        }

        for (prefix, value) in [
            ("apexshot://studio/activate?", encoded),
            ("apexshot://studio/activate/?", padded.clone()),
            ("apexshot://studio/activate?", padded.replace('=', "%3D")),
        ] {
            let path = scratch_path();
            let _scope = test_entitlement_path_scope(path.clone());
            let url = format!("{prefix}token={value}");
            assert_eq!(activate_from_url(&url), Ok(()));
            assert_eq!(
                current_status(),
                StudioStatus::Active {
                    updates_until: 3_000_000_000
                }
            );
            let _ = fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn malformed_activation_links_are_rejected_without_writing() {
        let path = scratch_path();
        let _scope = test_entitlement_path_scope(path.clone());
        let oversized = "A".repeat(MAX_TOKEN_BYTES * 2);
        for url in [
            "https://studio/activate?token=e30".to_string(),
            "apexshot://studio/purchase?token=e30".to_string(),
            "apexshot://studio/activate".to_string(),
            "apexshot://studio/activate?from=app".to_string(),
            "apexshot://studio/activate?token=".to_string(),
            "apexshot://studio/activate?token=not*base64".to_string(),
            "apexshot://studio/activate?token=e30&from=app".to_string(),
            "apexshot://studio/activate?token=e30#fragment".to_string(),
            "apexshot://studio/activate?token=%ZZ".to_string(),
            format!("apexshot://studio/activate?token={oversized}"),
        ] {
            assert_eq!(
                activate_from_url(&url),
                Err(StudioEntitlementError::InvalidEntitlement),
                "{url}"
            );
            assert!(!path.exists());
        }
    }
}
