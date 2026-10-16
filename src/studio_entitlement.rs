use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use crate::capture::editor::image_transform::StaticImageTransform;
use crate::recording::editor::model::{VideoEditState, ZoomCameraSettings, ZoomClip};

const KEY_ID: &str = "studio-v1";
const PRODUCT: &str = "apexshot-studio";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
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
}

impl StudioCapabilities {
    pub fn requires_studio(self) -> bool {
        self.motion_export || self.static_presentation || self.advanced_zoom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioStatus {
    ConfigurationUnavailable,
    NotActivated,
    InvalidEntitlement,
    Active { updates_until: i64 },
    UpdateAccessEnded { updates_until: i64 },
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StudioEntitlementError {
    #[error("Studio activation is unavailable in this build configuration")]
    ConfigurationUnavailable,
    #[error("Studio isn't activated on this device")]
    NotActivated,
    #[error("The Studio entitlement is invalid or could not be verified")]
    InvalidEntitlement,
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
        Err(_) => StudioStatus::InvalidEntitlement,
    }
}

pub fn activate_token(token: &str) -> Result<(), StudioEntitlementError> {
    let configuration = build_configuration()?;
    let path = entitlement_path()?;
    activate_token_at_path(&path, token, &configuration)
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
    verify_token(token, &configuration.verifying_key)?;
    write_token_atomically(path, token)
}

pub fn ensure_export_allowed(
    capabilities: StudioCapabilities,
) -> Result<(), StudioEntitlementError> {
    if !capabilities.requires_studio() {
        return Ok(());
    }
    let configuration = build_configuration()?;
    let path = entitlement_path()?;
    let entitlement =
        read_entitlement(&path, &configuration)?.ok_or(StudioEntitlementError::NotActivated)?;
    if !build_is_covered(configuration.release_timestamp, entitlement.updates_until) {
        return Err(StudioEntitlementError::UpdateAccessEnded);
    }
    Ok(())
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
    if entitlement.schema_version != 1
        || entitlement.product != PRODUCT
        || !is_uuid(&entitlement.license_id)
        || entitlement.issued_at <= 0
        || entitlement.updates_until < entitlement.issued_at
    {
        return Err(StudioEntitlementError::InvalidEntitlement);
    }
    Ok(entitlement)
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
    verify_token(token.trim(), &configuration.verifying_key).map(Some)
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
