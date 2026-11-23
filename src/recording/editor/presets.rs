//! Saved looks the editor can put on any recording.
//!
//! A preset is the part of an edit that is not a fact about the footage: the
//! stage, the cursor's settings and the click sound. Each one is a JSON file of
//! its own in a directory of its own, beside the saved projects and the pointer
//! sidecars, so a preset can never be read as either.
//!
//! The file holds no path, no timestamp and no trim. Applying a preset must
//! leave the recording it lands on exactly as it was: the trims, the cuts, the
//! zoom clips, the hidden-cursor clips and the media list stay with the
//! footage.
//!
//! The cursor's enum fields are stored as plain strings rather than as serde
//! enums. The shared model keeps its enums unserialisable on purpose, and a
//! preset is a document another build may read: the file spells what it means,
//! and a name this build does not know falls back to the default instead of
//! failing the whole preset.
//!
//! The stage also carries a Linux extension, [`StageFill`], so a plain colour
//! or a hand-drawn gradient round-trips here. A build that does not know the
//! field simply ignores it, and a file that build wrote has no `fill`, which
//! reads as the wallpaper named by `selection` or as no fill at all.
//!
//! Every save writes a complete file under a fresh name and renames it into
//! place, so a crash mid-write leaves the previous preset intact rather than a
//! half-written one the listing would ignore.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::model::{
    ClickEffect, CursorRotationMode, CursorSettings, CursorTheme, GradientStop, VideoBackground,
    VideoEditState, VideoGradient,
};
use super::project::{GradientKindFile, GradientStopFile, VideoProjectFile};
use super::sound::CLICK_SOFT;

/// Format version of the file. A reader accepts this version and nothing else:
/// a newer file was written by a build that knows more than this one, and
/// reading half of it would silently drop what the other half means.
const PRESET_VERSION: u32 = 1;

/// The largest file that can be a preset. A preset is a handful of numbers and
/// a short name; anything past this is not one, and reading it whole would let
/// a stray file in the directory cost a megabyte of memory per listing.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// Longest id, in bytes. The id is ASCII, so this is also its length in
/// characters, and it leaves room for the file name around it.
const MAX_ID_BYTES: usize = 80;

/// Longest name, in characters. Long enough for a sentence, short enough for a
/// card in a list.
const MAX_NAME_CHARS: usize = 120;

/// The top of the stage's padding and radius, in the Background panel's units
/// against a 400px long edge. The same ceiling the file format uses on the
/// other build, so a preset written here is one it can draw.
const MAX_STAGE_INSET: f64 = 100.0;

/// The stage's closing values when a stage was never captured, matching the
/// editor's own Background panel defaults.
const DEFAULT_PADDING: f64 = 24.0;
const DEFAULT_RADIUS: f64 = 0.0;

/// The volume a click sound opens at, matching the panel's own default.
const DEFAULT_CLICK_SOUND_VOLUME: f64 = 0.25;

/// The only click sound this editor ships, by its catalog name.
const SHIPPED_CLICK_SOUND: &str = CLICK_SOFT;

/// The cursor settings as a preset stores them.
///
/// One key per field of the shared model, under the model's own names, so a
/// reader can line the two up without a table. The enums are strings for the
/// reason the module explains; `click_color` stays a triple of channels
/// because that is what it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorFile {
    pub theme: String,
    pub size: f64,
    pub speed: f64,
    pub shadow: f64,
    pub smooth: f64,
    pub hide_idle: bool,
    pub idle_ms: f64,
    pub click_effect: String,
    pub click_intensity: f64,
    pub click_color: (u8, u8, u8),
    pub click_scale: f64,
    pub click_opacity: f64,
    pub click_duration_ms: u32,
    pub rotation_mode: String,
    pub rotate_on_x_movement_ratio: f64,
    pub base_rotation_deg: f64,
    pub always_use_default: bool,
    pub remove_shake_threshold: f64,
    pub stop_movement_in_last_part_ms: f64,
    pub loop_position_before_end_ms: Option<f64>,
}

impl Default for CursorFile {
    fn default() -> Self {
        Self::from_settings(CursorSettings::default())
    }
}

impl CursorFile {
    /// The settings as the file stores them.
    pub fn from_settings(settings: CursorSettings) -> Self {
        Self {
            theme: settings.theme.as_str().to_owned(),
            size: settings.size,
            speed: settings.speed,
            shadow: settings.shadow,
            smooth: settings.smooth,
            hide_idle: settings.hide_idle,
            idle_ms: settings.idle_ms,
            click_effect: click_effect_name(settings.click_effect).to_owned(),
            click_intensity: settings.click_intensity,
            click_color: settings.click_color,
            click_scale: settings.click_scale,
            click_opacity: settings.click_opacity,
            click_duration_ms: settings.click_duration_ms,
            rotation_mode: settings.rotation_mode.as_str().to_owned(),
            rotate_on_x_movement_ratio: settings.rotate_on_x_movement_ratio,
            base_rotation_deg: settings.base_rotation_deg,
            always_use_default: settings.always_use_default,
            remove_shake_threshold: settings.remove_shake_threshold,
            stop_movement_in_last_part_ms: settings.stop_movement_in_last_part_ms,
            loop_position_before_end_ms: settings.loop_position_before_end_ms,
        }
    }

    /// The settings a file describes.
    ///
    /// A number that is not a number reads as the default before the shared
    /// clamp runs: the clamp compares, and comparing against a NaN carries it
    /// straight through into the renderer.
    pub fn settings(&self) -> CursorSettings {
        let defaults = CursorSettings::default();
        CursorSettings {
            theme: CursorTheme::parse(&self.theme),
            size: finite_or(self.size, defaults.size),
            speed: finite_or(self.speed, defaults.speed),
            shadow: finite_or(self.shadow, defaults.shadow),
            smooth: finite_or(self.smooth, defaults.smooth),
            hide_idle: self.hide_idle,
            idle_ms: finite_or(self.idle_ms, defaults.idle_ms),
            click_effect: click_effect_from_name(&self.click_effect),
            click_intensity: finite_or(self.click_intensity, defaults.click_intensity),
            click_color: self.click_color,
            click_scale: finite_or(self.click_scale, defaults.click_scale),
            click_opacity: finite_or(self.click_opacity, defaults.click_opacity),
            click_duration_ms: self.click_duration_ms,
            rotation_mode: CursorRotationMode::parse(&self.rotation_mode),
            rotate_on_x_movement_ratio: finite_or(
                self.rotate_on_x_movement_ratio,
                defaults.rotate_on_x_movement_ratio,
            ),
            base_rotation_deg: finite_or(self.base_rotation_deg, defaults.base_rotation_deg),
            always_use_default: self.always_use_default,
            remove_shake_threshold: finite_or(
                self.remove_shake_threshold,
                defaults.remove_shake_threshold,
            ),
            stop_movement_in_last_part_ms: finite_or(
                self.stop_movement_in_last_part_ms,
                defaults.stop_movement_in_last_part_ms,
            ),
            loop_position_before_end_ms: self
                .loop_position_before_end_ms
                .filter(|value| value.is_finite()),
        }
        .clamped()
    }
}

fn click_effect_name(effect: ClickEffect) -> &'static str {
    match effect {
        ClickEffect::None => "none",
        ClickEffect::Spotlight => "spotlight",
        ClickEffect::Ripple => "ripple",
        ClickEffect::Circle => "circle",
        ClickEffect::Echo => "echo",
    }
}

fn click_effect_from_name(name: &str) -> ClickEffect {
    match name.trim().to_ascii_lowercase().as_str() {
        "none" => ClickEffect::None,
        "spotlight" => ClickEffect::Spotlight,
        "circle" => ClickEffect::Circle,
        "echo" => ClickEffect::Echo,
        _ => ClickEffect::Ripple,
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// The stage a preset puts behind the video: the wallpaper by its bundled
/// catalog name, the inset around the video, the card's corner radius, and a
/// Linux-only fill for a plain colour or a hand-drawn gradient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetStage {
    pub selection: Option<String>,
    pub padding: f64,
    pub radius: f64,
    /// The Linux extension. A build that does not know it ignores the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<StageFill>,
}

impl Default for PresetStage {
    fn default() -> Self {
        Self {
            selection: None,
            padding: DEFAULT_PADDING,
            radius: DEFAULT_RADIUS,
            fill: None,
        }
    }
}

/// A stage fill the bundled catalog does not describe: a flat colour or a
/// hand-drawn gradient, stored with the same shape a project file uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StageFill {
    Plain {
        r: u8,
        g: u8,
        b: u8,
    },
    Gradient {
        #[serde(default)]
        stops: Vec<GradientStopFile>,
        #[serde(default)]
        angle_degrees: f64,
        #[serde(default)]
        reversed: bool,
        #[serde(default)]
        kind: GradientKindFile,
    },
}

/// The keyboard captions a preset captured. Linux does not draw them, but a
/// file this build writes says so plainly rather than leaving the key out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptionsFile {
    pub shortcuts: bool,
    pub show_single_letters: bool,
}

/// One saved look.
///
/// `version`, `id` and `name` are the file's identity and have no default: a
/// file missing any of them is not a preset this build can name, so it is not
/// one it reads. Everything the look is made of reads as its panel default
/// when absent, which is what lets a preset grow a field without every saved
/// file being rewritten.
///
/// `camera` and `captions` are carried only so a file written by the other
/// build reads back here: this editor has no camera layer and no keystroke
/// captions, so it writes `camera: null` and default captions and ignores them
/// when one is read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub stage: PresetStage,
    #[serde(default)]
    pub cursor: CursorFile,
    #[serde(default)]
    pub camera: Option<serde_json::Value>,
    #[serde(default)]
    pub captions: CaptionsFile,
    /// The click sound by its catalog name; only the shipped one survives.
    #[serde(default)]
    pub click_sound: Option<String>,
    #[serde(default = "default_click_sound_volume")]
    pub click_sound_volume: f64,
}

fn default_click_sound_volume() -> f64 {
    DEFAULT_CLICK_SOUND_VOLUME
}

impl Preset {
    /// This preset with everything a reader can draw: the version it writes,
    /// the trimmed name, a stage from the bundled catalog, finite and in-range
    /// numbers, and the click sound this editor ships.
    fn sanitized(&self) -> Self {
        Self {
            version: PRESET_VERSION,
            id: self.id.clone(),
            name: self.name.trim().to_owned(),
            stage: sanitized_stage(&self.stage),
            cursor: CursorFile::from_settings(self.cursor.settings()),
            camera: None,
            captions: CaptionsFile::default(),
            click_sound: shipped_click_sound(self.click_sound.as_deref()),
            click_sound_volume: finite_or(self.click_sound_volume, DEFAULT_CLICK_SOUND_VOLUME)
                .clamp(0.0, 1.0),
        }
    }
}

fn sanitized_stage(stage: &PresetStage) -> PresetStage {
    PresetStage {
        selection: catalog_selection(stage.selection.as_deref()),
        padding: finite_or(stage.padding, DEFAULT_PADDING).clamp(0.0, MAX_STAGE_INSET),
        radius: finite_or(stage.radius, DEFAULT_RADIUS).clamp(0.0, MAX_STAGE_INSET),
        fill: sanitized_fill(stage.fill.as_ref()),
    }
}

/// The bundled wallpaper catalog, the same sixty names the Background panel
/// draws and the other build stores by name.
fn wallpaper_catalog() -> impl Iterator<Item = &'static str> {
    crate::capture::editor::window::background_panel::MOTION_WALLPAPER_FILES
        .iter()
        .copied()
        .filter(|name| name.starts_with("wallpaper-"))
}

fn wallpaper_asset_path(name: &str) -> PathBuf {
    crate::capture::editor::window::background_panel::background_gradient_asset_path(name)
}

fn catalog_selection(selection: Option<&str>) -> Option<String> {
    let name = selection?;
    wallpaper_catalog()
        .find(|candidate| *candidate == name)
        .map(|candidate| candidate.to_owned())
}

fn bundled_wallpaper_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    catalog_selection(Some(name))
}

fn shipped_click_sound(sound: Option<&str>) -> Option<String> {
    sound
        .filter(|sound| *sound == SHIPPED_CLICK_SOUND)
        .map(|sound| sound.to_owned())
}

fn gradient_from_fill(
    stops: &[GradientStopFile],
    angle_degrees: f64,
    reversed: bool,
    kind: GradientKindFile,
) -> VideoGradient {
    VideoGradient {
        kind: kind.into(),
        stops: stops
            .iter()
            .map(|stop| GradientStop::rgba(stop.position, stop.r, stop.g, stop.b, stop.a))
            .collect(),
        angle_degrees,
        reversed,
    }
    .normalized()
}

fn fill_from_gradient(gradient: &VideoGradient) -> StageFill {
    let gradient = gradient.normalized();
    StageFill::Gradient {
        stops: gradient
            .stops
            .iter()
            .map(|stop| GradientStopFile {
                position: stop.position,
                r: stop.r,
                g: stop.g,
                b: stop.b,
                a: stop.a,
            })
            .collect(),
        angle_degrees: gradient.angle_degrees,
        reversed: gradient.reversed,
        kind: gradient.kind.into(),
    }
}

fn sanitized_fill(fill: Option<&StageFill>) -> Option<StageFill> {
    match fill? {
        StageFill::Plain { r, g, b } => Some(StageFill::Plain {
            r: *r,
            g: *g,
            b: *b,
        }),
        StageFill::Gradient {
            stops,
            angle_degrees,
            reversed,
            kind,
        } => Some(fill_from_gradient(&gradient_from_fill(
            stops,
            *angle_degrees,
            *reversed,
            *kind,
        ))),
    }
}

/// The stage a background describes: a bundled wallpaper by name, or the
/// Linux fill for a colour and a gradient. A user-picked image is no stage at
/// all, because its path belongs to one machine.
fn stage_from_background(background: &VideoBackground, padding: f64, radius: f64) -> PresetStage {
    let (selection, fill) = match background {
        VideoBackground::None => (None, None),
        VideoBackground::Plain { r, g, b } => (
            None,
            Some(StageFill::Plain {
                r: *r,
                g: *g,
                b: *b,
            }),
        ),
        VideoBackground::Gradient(gradient) => (None, Some(fill_from_gradient(gradient))),
        VideoBackground::Wallpaper(path) => (bundled_wallpaper_name(path), None),
    };
    PresetStage {
        selection,
        padding,
        radius,
        fill,
    }
}

/// The background a sanitized stage draws. The wallpaper name wins over the
/// fill, which is how the shared file reads too.
fn background_for_stage(stage: &PresetStage) -> VideoBackground {
    if let Some(name) = stage
        .selection
        .as_deref()
        .and_then(|name| catalog_selection(Some(name)))
    {
        return VideoBackground::Wallpaper(wallpaper_asset_path(&name));
    }
    match &stage.fill {
        Some(StageFill::Plain { r, g, b }) => VideoBackground::Plain {
            r: *r,
            g: *g,
            b: *b,
        },
        Some(StageFill::Gradient {
            stops,
            angle_degrees,
            reversed,
            kind,
        }) => {
            VideoBackground::Gradient(gradient_from_fill(stops, *angle_degrees, *reversed, *kind))
        }
        None => VideoBackground::None,
    }
}

/// Capture the look of an open edit as a new preset.
///
/// The stage, the cursor's settings and the click sound come across; the
/// source, the trims, the cuts, the zoom clips, the hidden-cursor clips, the
/// playhead and the media list stay behind.
///
/// The id is minted here rather than taken from the name: the name is the
/// user's text and can be anything, while the id names a file. Two captures in
/// the same instant still differ, because the clock is only one part of it and
/// the sequence below is process-wide.
pub fn from_state(state: &VideoEditState, name: &str) -> Result<Preset, String> {
    from_project(&state.to_project(), state.cursor, name)
}

/// Capture a look from a project file this build just wrote, so a caller that
/// already has the persisted shape does not have to reach into the state.
pub fn from_project(
    file: &VideoProjectFile,
    cursor: CursorSettings,
    name: &str,
) -> Result<Preset, String> {
    let background = super::project::background_from_file(file.background.clone());
    captured(
        &background,
        file.background_padding,
        file.background_corner_radius,
        cursor,
        file.sound.click_sound.clone(),
        file.sound.click_sound_volume,
        name,
    )
}

fn captured(
    background: &VideoBackground,
    padding: f64,
    radius: f64,
    cursor: CursorSettings,
    click_sound: Option<String>,
    click_sound_volume: f64,
    name: &str,
) -> Result<Preset, String> {
    let preset = Preset {
        version: PRESET_VERSION,
        id: generated_id(),
        name: name.to_owned(),
        stage: stage_from_background(background, padding, radius),
        cursor: CursorFile::from_settings(cursor),
        camera: None,
        captions: CaptionsFile::default(),
        click_sound,
        click_sound_volume,
    };
    validate(&preset)?;
    Ok(preset.sanitized())
}

fn generated_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!(
        "preset-{}-{nanos}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// Put a preset's look on an open edit.
///
/// Only the stage, the cursor and the click sound are written. The trims, the
/// cuts, the zooms, the hidden-cursor clips, the media list and the title stay
/// with the recording, so one call is the whole change.
pub fn apply_to_state(state: &mut VideoEditState, preset: &Preset) {
    let preset = preset.sanitized();
    state.background = background_for_stage(&preset.stage);
    state.background_padding = preset.stage.padding;
    state.background_corner_radius = preset.stage.radius;
    state.cursor = preset.cursor.settings();
    state.sound.click_sound = preset.click_sound;
    state.sound.click_sound_volume = preset.click_sound_volume;
    // The cursor's shake filter and tail behaviours reach the pointer plan
    // through the sidecar, exactly as the Cursor panel's own writes do.
    state.sync_sidecar_cursor_filters();
}

/// Whether the open edit still carries `preset`'s look.
///
/// The preset popover has no history listener, so it asks this when it opens:
/// an undo, a document swap or a later edit that moved the look must drop the
/// row's applied highlight.
pub fn look_matches(state: &VideoEditState, preset: &Preset) -> bool {
    let preset = preset.sanitized();
    let stage = sanitized_stage(&stage_from_background(
        &state.background,
        state.background_padding,
        state.background_corner_radius,
    ));
    stage == preset.stage
        && CursorFile::from_settings(state.cursor.clamped()) == preset.cursor
        && state.sound.click_sound == preset.click_sound
        && (state.sound.click_sound_volume - preset.click_sound_volume).abs() <= f64::EPSILON
}

/// Put the configured default preset on a recording that has no saved project
/// yet, so every new recording opens on the look the user chose.
///
/// A missing, moved, deleted or corrupted preset reads as none, which leaves
/// the built-in look rather than failing the open.
pub fn apply_default_preset(state: &mut VideoEditState) {
    let Some(id) = crate::config::load_config().rec_video_default_preset else {
        return;
    };
    let Some(preset) = load(&id) else {
        return;
    };
    apply_to_state(state, &preset);
}

/// Where saved presets live: beside the saved projects and the pointer
/// sidecars, under the same local data root, in a directory of its own. The
/// path and the file format are shared with the other build, so one store
/// serves both.
fn directory() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| root.join("apexshot").join("video-presets"))
}

/// Every preset that reads, ordered by name and then by id.
pub fn list() -> Vec<Preset> {
    match directory() {
        Some(directory) => list_at(&directory),
        None => Vec::new(),
    }
}

/// One preset by id.
pub fn load(id: &str) -> Option<Preset> {
    load_at(&directory()?, id)
}

/// Save a preset under its own id, replacing a preset of the same id.
pub fn save(preset: &Preset) -> Result<(), String> {
    let Some(directory) = directory() else {
        return Err("no local data directory to keep presets in".to_owned());
    };
    save_at(&directory, preset)
}

/// Remove a saved preset's file.
pub fn delete(id: &str) -> Result<(), String> {
    let Some(directory) = directory() else {
        return Err("no local data directory to keep presets in".to_owned());
    };
    delete_at(&directory, id)
}

/// Where a saved preset's file is, for showing it in the file manager.
pub fn path_of(id: &str) -> Option<PathBuf> {
    directory()
        .filter(|_| valid_id(id))
        .map(|directory| file_path(&directory, id))
}

/// The preset a capture becomes when it replaces `existing`: the same id and
/// name, so the list keeps its place and the name the user gave stays.
pub fn replacing(existing: &Preset, captured: Preset) -> Preset {
    Preset {
        id: existing.id.clone(),
        name: existing.name.clone(),
        ..captured
    }
}

fn file_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(format!("{id}.json"))
}

fn list_at(directory: &Path) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut presets: Vec<Preset> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_owned();
            read_preset(&path, &id)
        })
        .collect();
    presets.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    presets
}

fn load_at(directory: &Path, id: &str) -> Option<Preset> {
    if !valid_id(id) {
        return None;
    }
    read_preset(&file_path(directory, id), id)
}

fn read_preset(path: &Path, id: &str) -> Option<Preset> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let json = std::fs::read_to_string(path).ok()?;
    let preset: Preset = match serde_json::from_str(&json) {
        Ok(preset) => preset,
        Err(err) => {
            eprintln!(
                "[recording-editor] failed to parse preset {}: {err}",
                path.display()
            );
            return None;
        }
    };
    if preset.id != id {
        return None;
    }
    validate(&preset).ok()?;
    Some(preset.sanitized())
}

fn save_at(directory: &Path, preset: &Preset) -> Result<(), String> {
    validate(preset)?;
    let preset = preset.sanitized();
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("could not create {}: {err}", directory.display()))?;
    let json = serde_json::to_string_pretty(&preset)
        .map_err(|err| format!("could not write preset {}: {err}", preset.id))?;
    let path = file_path(directory, &preset.id);
    let temp = unique_temp_path(directory, &preset.id);
    write_temp(&temp, json.as_bytes())?;
    if let Err(err) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("could not replace {}: {err}", path.display()));
    }
    Ok(())
}

fn delete_at(directory: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("{id:?} is not a preset id"));
    }
    let path = file_path(directory, id);
    std::fs::remove_file(&path).map_err(|err| format!("could not delete {}: {err}", path.display()))
}

/// Write `bytes` to a file this call creates and owns.
///
/// The open is the only step that can fail before there is a file to clean up,
/// so a path this call did not create is never removed. Everything after it
/// removes the file this call made and nothing else, which a competing writer
/// in the same directory could otherwise lose.
fn write_temp(temp: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut handle = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(|err| format!("could not create {}: {err}", temp.display()))?;
    if let Err(err) = handle.write_all(bytes) {
        drop(handle);
        let _ = std::fs::remove_file(temp);
        return Err(format!("could not write {}: {err}", temp.display()));
    }
    if let Err(err) = handle.sync_all() {
        drop(handle);
        let _ = std::fs::remove_file(temp);
        return Err(format!("could not flush {}: {err}", temp.display()));
    }
    Ok(())
}

fn unique_temp_path(directory: &Path, id: &str) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    directory.join(format!(
        "{id}.{}-{nanos}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn validate(preset: &Preset) -> Result<(), String> {
    if preset.version != PRESET_VERSION {
        return Err(format!(
            "preset {} is version {}, and this build reads version {PRESET_VERSION}",
            preset.id, preset.version
        ));
    }
    if !valid_id(&preset.id) {
        return Err(format!("{:?} is not a preset id", preset.id));
    }
    if !valid_name(&preset.name) {
        return Err(format!("preset {} has no usable name", preset.id));
    }
    Ok(())
}

/// An id names a file, so it is ASCII letters, digits, `-` and `_` and nothing
/// else. That set has no separator, no drive letter, no dot and no byte a
/// filesystem might interpret, which leaves a traversal nothing to work with.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= MAX_NAME_CHARS
        && !trimmed.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::{
        DimensionPreset, EditorTool, ExportQuality, VideoMetadata,
    };
    use std::fs;

    const CURSOR_KEYS: [&str; 20] = [
        "always_use_default",
        "base_rotation_deg",
        "click_color",
        "click_duration_ms",
        "click_effect",
        "click_intensity",
        "click_opacity",
        "click_scale",
        "hide_idle",
        "idle_ms",
        "loop_position_before_end_ms",
        "remove_shake_threshold",
        "rotate_on_x_movement_ratio",
        "rotation_mode",
        "shadow",
        "size",
        "smooth",
        "speed",
        "stop_movement_in_last_part_ms",
        "theme",
    ];

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-linux-presets-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn state() -> VideoEditState {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/apexshot-presets-fixture.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 4096,
            has_audio: false,
            frame_rate: 30.0,
        });
        state.trim_start_seconds = 1.0;
        state.trim_end_seconds = 9.5;
        state.cuts = vec![3.0, 6.0];
        state.segments_kept = vec![true, true, true];
        state.segment_order = vec![0, 1, 2];
        state.segment_starts = vec![0.0, 0.0, 0.0];
        state.segment_speeds = vec![1.0, 1.0, 1.0];
        state.segment_muted = vec![false, false, false];
        state.background = VideoBackground::Wallpaper(wallpaper_asset_path("wallpaper-004.jpg"));
        state.background_padding = 40.0;
        state.background_corner_radius = 5.0;
        state.sound.click_sound = Some(CLICK_SOFT.to_owned());
        state.sound.click_sound_volume = 0.6;
        state.dimension_preset = DimensionPreset::P720;
        state.quality = ExportQuality::Ultra;
        state.selected_tool = EditorTool::Background;
        state
    }

    fn tuned_cursor() -> CursorSettings {
        CursorSettings {
            theme: CursorTheme::TahoeInverted,
            size: 1.75,
            speed: 0.5,
            shadow: 0.9,
            smooth: 0.65,
            hide_idle: true,
            idle_ms: 2400.0,
            click_effect: ClickEffect::Echo,
            click_intensity: 0.35,
            click_color: (12, 34, 56),
            click_scale: 1.4,
            click_opacity: 0.8,
            click_duration_ms: 640,
            rotation_mode: CursorRotationMode::Directional,
            rotate_on_x_movement_ratio: 0.25,
            base_rotation_deg: -37.5,
            always_use_default: true,
            remove_shake_threshold: 45.0,
            stop_movement_in_last_part_ms: 1200.0,
            loop_position_before_end_ms: Some(750.0),
        }
    }

    fn preset_named(id: &str, name: &str) -> Preset {
        Preset {
            version: PRESET_VERSION,
            id: id.to_owned(),
            name: name.to_owned(),
            stage: PresetStage::default(),
            cursor: CursorFile::default(),
            camera: None,
            captions: CaptionsFile::default(),
            click_sound: None,
            click_sound_volume: DEFAULT_CLICK_SOUND_VOLUME,
        }
    }

    #[test]
    fn a_preset_round_trips_through_its_file() {
        let directory = scratch("round-trip");
        let mut source = state();
        source.cursor = tuned_cursor();
        let preset = from_state(&source, "Look one").unwrap();
        save_at(&directory, &preset).unwrap();

        let path = directory.join(format!("{}.json", preset.id));
        assert!(path.is_file(), "a preset is one json file named by its id");

        let loaded = load_at(&directory, &preset.id).expect("the preset loads");
        assert_eq!(loaded, preset.sanitized());
        assert_eq!(loaded.cursor.settings(), tuned_cursor());
        assert_eq!(loaded.name, "Look one");
        assert_eq!(list_at(&directory), vec![loaded]);
    }

    #[test]
    fn a_cursor_file_states_every_cursor_setting() {
        let value = serde_json::to_value(CursorFile::default()).unwrap();
        let object = value.as_object().expect("a cursor file is an object");
        assert_eq!(object.len(), CURSOR_KEYS.len());
        for key in CURSOR_KEYS {
            assert!(object.contains_key(key), "the cursor file states {key}");
        }
    }

    #[test]
    fn every_cursor_setting_round_trips_through_the_file() {
        let tuned = tuned_cursor();
        let file = CursorFile::from_settings(tuned);
        assert_eq!(file.theme, "tahoe_inverted");
        assert_eq!(file.click_effect, "echo");
        assert_eq!(file.rotation_mode, "directional");
        assert_eq!(file.settings(), tuned);

        let text = serde_json::to_string(&file).unwrap();
        let loaded: CursorFile = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded, file);
        assert_eq!(loaded.settings(), tuned);
    }

    #[test]
    fn every_click_effect_has_its_own_name() {
        for effect in [
            ClickEffect::None,
            ClickEffect::Spotlight,
            ClickEffect::Ripple,
            ClickEffect::Circle,
            ClickEffect::Echo,
        ] {
            assert_eq!(click_effect_from_name(click_effect_name(effect)), effect);
        }
        assert_eq!(click_effect_from_name("Spotlight"), ClickEffect::Spotlight);
        assert_eq!(click_effect_from_name("ripple"), ClickEffect::Ripple);
        assert_eq!(click_effect_from_name("pulse"), ClickEffect::Ripple);
    }

    #[test]
    fn unknown_cursor_names_fall_back_to_the_defaults() {
        let cursor: CursorFile = serde_json::from_str(
            r#"{"theme":"nonsense","click_effect":"nonsense","rotation_mode":"sideways"}"#,
        )
        .unwrap();
        let settings = cursor.settings();
        assert_eq!(settings.theme, CursorTheme::Adwaita);
        assert_eq!(settings.click_effect, ClickEffect::Ripple);
        assert_eq!(settings.rotation_mode, CursorRotationMode::Velocity);
        assert_eq!(settings, CursorSettings::default());
    }

    #[test]
    fn an_unusable_cursor_number_reads_as_the_default_before_clamping() {
        let broken = CursorFile {
            size: f64::NAN,
            speed: f64::INFINITY,
            shadow: f64::NEG_INFINITY,
            smooth: f64::NAN,
            idle_ms: f64::NAN,
            click_intensity: f64::NAN,
            click_scale: f64::NAN,
            click_opacity: f64::NAN,
            click_duration_ms: 9_000,
            rotate_on_x_movement_ratio: f64::NAN,
            base_rotation_deg: f64::NAN,
            remove_shake_threshold: f64::NAN,
            stop_movement_in_last_part_ms: f64::NAN,
            loop_position_before_end_ms: Some(f64::NAN),
            ..CursorFile::default()
        };
        let defaults = CursorSettings::default();
        let settings = broken.settings();
        assert_eq!(settings.size, defaults.size);
        assert_eq!(settings.speed, defaults.speed);
        assert_eq!(settings.shadow, defaults.shadow);
        assert_eq!(settings.smooth, defaults.smooth);
        assert_eq!(settings.idle_ms, defaults.idle_ms);
        assert_eq!(settings.click_intensity, defaults.click_intensity);
        assert_eq!(settings.click_scale, defaults.click_scale);
        assert_eq!(settings.click_opacity, defaults.click_opacity);
        assert_eq!(
            settings.rotate_on_x_movement_ratio,
            defaults.rotate_on_x_movement_ratio
        );
        assert_eq!(settings.base_rotation_deg, defaults.base_rotation_deg);
        assert_eq!(
            settings.remove_shake_threshold,
            defaults.remove_shake_threshold
        );
        assert_eq!(settings.stop_movement_in_last_part_ms, 0.0);
        assert_eq!(settings.loop_position_before_end_ms, None);
        assert_eq!(settings.click_duration_ms, 1_200);
    }

    #[test]
    fn a_captured_preset_carries_no_source_fact_or_timeline() {
        let source = state();
        let before = source.clone();
        let preset = from_state(&source, "Look").unwrap();
        assert_eq!(
            source.trim_start_seconds, before.trim_start_seconds,
            "capturing leaves the edit untouched"
        );

        let json = serde_json::to_string(&preset).unwrap();
        for key in [
            "source_path",
            "source_size",
            "source_mtime_secs",
            "trim_start_seconds",
            "trim_end_seconds",
            "cuts",
            "segment_starts",
            "segment_speeds",
            "zoom_clips",
            "zoom_camera",
            "cursor_hide_clips",
            "playhead_seconds",
            "timeline_scale",
            "timeline_scroll_seconds",
            "extra_media",
        ] {
            assert!(
                !json.contains(&format!("\"{key}\"")),
                "a preset carries no {key}"
            );
        }
        assert!(
            json.contains("\"camera\":null"),
            "a Linux preset states it has no camera"
        );
        assert!(json.contains("\"captions\""));
        assert!(valid_id(&preset.id));
        assert_eq!(preset.stage.selection.as_deref(), Some("wallpaper-004.jpg"));
        assert_eq!(preset.stage.padding, 40.0);
        assert_eq!(preset.stage.radius, 5.0);
        assert_eq!(preset.click_sound.as_deref(), Some("soft"));
        assert_eq!(preset.click_sound_volume, 0.6);
    }

    #[test]
    fn two_captures_get_their_own_ids() {
        let source = state();
        let first = from_state(&source, "Look").unwrap();
        let second = from_state(&source, "Look").unwrap();
        assert_ne!(first.id, second.id);
        assert!(valid_id(&first.id));
        assert!(valid_id(&second.id));
        assert!(first.id.len() <= MAX_ID_BYTES);
    }

    #[test]
    fn a_capture_needs_a_name_this_editor_can_show() {
        let source = state();
        assert!(from_state(&source, "   ").is_err());
        assert!(from_state(&source, "two\nlines").is_err());
        assert!(from_state(&source, &"n".repeat(121)).is_err());
        assert!(from_state(&source, &"n".repeat(120)).is_ok());

        let mut preset = preset_named("named", "  Look  ");
        save_at(&scratch("names"), &preset).unwrap();
        preset.name = "   ".to_owned();
        assert!(save_at(&scratch("names"), &preset).is_err());
    }

    #[test]
    fn applying_a_preset_writes_its_stage_cursor_and_click_sound() {
        let mut source = state();
        source.background = VideoBackground::Wallpaper(wallpaper_asset_path("wallpaper-019.jpg"));
        source.background_padding = 66.0;
        source.background_corner_radius = 9.0;
        source.cursor = tuned_cursor();
        source.sound.click_sound = None;
        source.sound.click_sound_volume = DEFAULT_CLICK_SOUND_VOLUME;
        let preset = from_state(&source, "Look").unwrap();

        let mut target = state();
        apply_to_state(&mut target, &preset);

        assert_eq!(
            target.background,
            VideoBackground::Wallpaper(wallpaper_asset_path("wallpaper-019.jpg"))
        );
        assert_eq!(target.background_padding, 66.0);
        assert_eq!(target.background_corner_radius, 9.0);
        assert_eq!(target.cursor, tuned_cursor());
        assert_eq!(target.sound.click_sound, None);
        assert_eq!(target.sound.click_sound_volume, DEFAULT_CLICK_SOUND_VOLUME);
    }

    #[test]
    fn applying_a_preset_leaves_every_recording_fact_alone() {
        let mut target = state();
        let before = target.clone();
        let preset = from_state(&before, "Look").unwrap();
        apply_to_state(&mut target, &preset);

        assert_eq!(target.metadata.path, before.metadata.path);
        assert_eq!(target.title, before.title);
        assert_eq!(target.trim_start_seconds, before.trim_start_seconds);
        assert_eq!(target.trim_end_seconds, before.trim_end_seconds);
        assert_eq!(target.cuts, before.cuts);
        assert_eq!(target.segments_kept, before.segments_kept);
        assert_eq!(target.segment_order, before.segment_order);
        assert_eq!(target.segment_starts, before.segment_starts);
        assert_eq!(target.segment_speeds, before.segment_speeds);
        assert_eq!(target.freeze_tail, before.freeze_tail);
        assert_eq!(target.frozen_segment, before.frozen_segment);
        assert_eq!(target.zoom_clips, before.zoom_clips);
        assert_eq!(target.zoom_camera, before.zoom_camera);
        assert_eq!(target.cursor_hide_clips, before.cursor_hide_clips);
        assert_eq!(target.playhead_seconds, before.playhead_seconds);
        assert_eq!(target.timeline_scale, before.timeline_scale);
        assert_eq!(
            target.timeline_scroll_seconds,
            before.timeline_scroll_seconds
        );
        assert_eq!(target.project_media, before.project_media);
        assert_eq!(target.crop, before.crop);
        assert_eq!(target.dimension_preset, before.dimension_preset);
        assert_eq!(target.quality, before.quality);
        assert_eq!(target.sound.volume, before.sound.volume);
        assert_eq!(
            target.sound.microphone_preset,
            before.sound.microphone_preset
        );
        assert_eq!(target.sound.background_file, before.sound.background_file);
    }

    #[test]
    fn only_the_shipped_click_sound_and_a_usable_volume_survive_a_save() {
        let directory = scratch("sound");
        let mut preset = preset_named("sound", "Sound");
        preset.click_sound = Some("hard".to_owned());
        preset.click_sound_volume = 4.0;
        save_at(&directory, &preset).unwrap();
        let loaded = load_at(&directory, "sound").expect("the preset loads");
        assert_eq!(loaded.click_sound, None);
        assert_eq!(loaded.click_sound_volume, 1.0);

        preset.click_sound = Some("soft".to_owned());
        preset.click_sound_volume = f64::NAN;
        save_at(&directory, &preset).unwrap();
        let loaded = load_at(&directory, "sound").expect("the preset loads");
        assert_eq!(loaded.click_sound.as_deref(), Some("soft"));
        assert_eq!(loaded.click_sound_volume, DEFAULT_CLICK_SOUND_VOLUME);
    }

    #[test]
    fn a_stage_holds_only_a_bundled_wallpaper_and_an_inset_it_can_draw() {
        let mut preset = preset_named("stage", "Stage");
        preset.stage = PresetStage {
            selection: Some("/home/me/wallpaper.jpg".to_owned()),
            padding: f64::NAN,
            radius: 900.0,
            fill: None,
        };
        let sanitized = preset.sanitized();
        assert_eq!(sanitized.stage.selection, None);
        assert_eq!(sanitized.stage.padding, DEFAULT_PADDING);
        assert_eq!(sanitized.stage.radius, MAX_STAGE_INSET);

        for selection in [
            "../wallpaper-004.jpg",
            "wallpaper-004.jpg/..",
            "wallpaper-004.png",
            "wallpa per-004.jpg",
            "wallpaper-061.jpg",
        ] {
            preset.stage.selection = Some(selection.to_owned());
            assert_eq!(preset.sanitized().stage.selection, None, "{selection}");
        }

        preset.stage.selection = Some("wallpaper-060.jpg".to_owned());
        assert_eq!(
            preset.sanitized().stage.selection.as_deref(),
            Some("wallpaper-060.jpg")
        );
    }

    #[test]
    fn a_custom_image_path_becomes_no_stage_at_all() {
        let mut source = state();
        source.background = VideoBackground::Wallpaper(PathBuf::from("/home/me/holiday.png"));
        let preset = from_state(&source, "Holiday").unwrap();
        assert_eq!(preset.stage.selection, None);
        assert_eq!(preset.stage.fill, None);

        let mut target = state();
        apply_to_state(&mut target, &preset);
        assert!(target.background.is_none());
    }

    #[test]
    fn linux_fills_round_trip_through_the_file() {
        let directory = scratch("fills");

        let mut plain = state();
        plain.background = VideoBackground::Plain {
            r: 12,
            g: 24,
            b: 36,
        };
        let preset = from_state(&plain, "Plain").unwrap();
        save_at(&directory, &preset).unwrap();
        let loaded = load_at(&directory, &preset.id).expect("the preset loads");
        assert_eq!(
            loaded.stage.fill,
            Some(StageFill::Plain {
                r: 12,
                g: 24,
                b: 36
            })
        );
        let mut target = state();
        apply_to_state(&mut target, &loaded);
        assert_eq!(
            target.background,
            VideoBackground::Plain {
                r: 12,
                g: 24,
                b: 36
            }
        );

        let gradient = VideoGradient {
            kind: crate::recording::editor::model::GradientKind::Radial,
            stops: vec![
                GradientStop::rgba(0.1, 10, 20, 30, 200),
                GradientStop::rgba(0.9, 200, 180, 160, 255),
            ],
            angle_degrees: 45.0,
            reversed: true,
        };
        let mut drawn = state();
        drawn.background = VideoBackground::Gradient(gradient.clone());
        let preset = from_state(&drawn, "Fade").unwrap();
        save_at(&directory, &preset).unwrap();
        let loaded = load_at(&directory, &preset.id).expect("the preset loads");
        let mut target = state();
        apply_to_state(&mut target, &loaded);
        assert_eq!(
            target.background,
            VideoBackground::Gradient(gradient.normalized())
        );
    }

    #[test]
    fn a_file_written_without_the_linux_fill_still_loads() {
        let directory = scratch("foreign");
        let id = "another-build-look";
        fs::write(
            directory.join(format!("{id}.json")),
            serde_json::json!({
                "version": 1,
                "id": id,
                "name": "  Studio  ",
                "stage": {
                    "selection": "wallpaper-004.jpg",
                    "padding": 48.0,
                    "radius": 6.0,
                },
                "cursor": { "theme": "tahoe", "size": 1.25 },
                "camera": null,
                "captions": { "shortcuts": true, "show_single_letters": true },
                "click_sound": "soft",
                "click_sound_volume": 0.4,
            })
            .to_string(),
        )
        .unwrap();

        let loaded = load_at(&directory, id).expect("a file without fill reads");
        assert_eq!(loaded.name, "Studio");
        assert_eq!(loaded.stage.selection.as_deref(), Some("wallpaper-004.jpg"));
        assert_eq!(loaded.stage.padding, 48.0);
        assert_eq!(loaded.stage.radius, 6.0);
        assert_eq!(loaded.stage.fill, None);
        assert_eq!(loaded.cursor.theme, "tahoe");
        assert_eq!(loaded.click_sound.as_deref(), Some("soft"));

        let mut target = state();
        apply_to_state(&mut target, &loaded);
        assert_eq!(
            target.background,
            VideoBackground::Wallpaper(wallpaper_asset_path("wallpaper-004.jpg"))
        );
        assert_eq!(target.background_padding, 48.0);
        assert_eq!(target.background_corner_radius, 6.0);
        assert_eq!(target.cursor.theme, CursorTheme::Tahoe);
    }

    #[test]
    fn a_preset_matches_only_the_look_it_captured() {
        let mut state = state();
        state.cursor = tuned_cursor();
        let preset = from_state(&state, "Look").unwrap();
        assert!(look_matches(&state, &preset));

        state.background_padding += 1.0;
        assert!(!look_matches(&state, &preset));

        state.background_padding -= 1.0;
        assert!(look_matches(&state, &preset));

        state.cursor.size = 2.0;
        assert!(!look_matches(&state, &preset));

        state.cursor = tuned_cursor();
        state.sound.click_sound_volume = 0.1;
        assert!(!look_matches(&state, &preset));
    }

    #[test]
    fn a_preset_id_can_only_name_a_file_in_the_preset_directory() {
        let directory = scratch("ids");
        let escape = directory
            .parent()
            .expect("a scratch directory has a parent")
            .join("escape.json");

        for id in [
            "../escape",
            "..\\escape",
            "a/b",
            "a\\b",
            "C:escape",
            "look:one",
            "",
            ".",
            "..",
            "dot.ted",
            "naive\u{0301}",
            "look one",
        ] {
            let mut preset = preset_named("safe", "Look");
            preset.id = id.to_owned();
            assert!(
                save_at(&directory, &preset).is_err(),
                "id {id:?} is refused"
            );
        }

        let mut long = preset_named("safe", "Look");
        long.id = "i".repeat(MAX_ID_BYTES + 1);
        assert!(save_at(&directory, &long).is_err());

        let mut longest = preset_named("safe", "Look");
        longest.id = "i".repeat(MAX_ID_BYTES);
        assert!(save_at(&directory, &longest).is_ok());

        assert!(load_at(&directory, "../escape").is_none());
        assert!(load_at(&directory, "").is_none());
        assert!(!escape.exists());
    }

    #[test]
    fn malformed_and_foreign_preset_files_are_ignored() {
        let directory = scratch("corrupt");
        save_at(&directory, &preset_named("good-one", "Good")).unwrap();
        fs::write(directory.join("broken.json"), b"{not json").unwrap();
        fs::write(
            directory.join("future.json"),
            serde_json::json!({ "version": 2, "id": "future", "name": "Future" }).to_string(),
        )
        .unwrap();
        fs::write(
            directory.join("mismatch.json"),
            serde_json::json!({ "version": 1, "id": "elsewhere", "name": "Misplaced" }).to_string(),
        )
        .unwrap();
        fs::write(
            directory.join("nameless.json"),
            serde_json::json!({ "version": 1, "id": "nameless", "name": "   " }).to_string(),
        )
        .unwrap();
        fs::write(
            directory.join("huge.json"),
            vec![b'{'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        fs::write(directory.join("notes.txt"), b"not a preset").unwrap();
        fs::create_dir_all(directory.join("folder.json")).unwrap();

        let listed = list_at(&directory);
        assert_eq!(listed.len(), 1, "only the readable preset is listed");
        assert_eq!(listed[0].id, "good-one");

        for id in ["broken", "future", "mismatch", "nameless", "huge", "folder"] {
            assert!(load_at(&directory, id).is_none(), "{id} does not read");
        }

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_absent_directory_reads_as_no_presets() {
        let directory = scratch("absent");
        let absent = directory.join("never-created");
        assert!(list_at(&absent).is_empty());
        assert!(load_at(&absent, "look-one").is_none());
        assert!(!absent.exists(), "listing never creates the directory");
    }

    #[test]
    fn presets_are_listed_by_name_ignoring_case() {
        let directory = scratch("listing");
        for (id, name) in [("b-one", "beta"), ("a-two", "Alpha"), ("c-three", "alpha")] {
            save_at(&directory, &preset_named(id, name)).unwrap();
        }
        let listed = list_at(&directory);
        let ids: Vec<&str> = listed.iter().map(|preset| preset.id.as_str()).collect();
        assert_eq!(ids, ["a-two", "c-three", "b-one"]);
    }

    #[test]
    fn saving_over_a_preset_replaces_it_in_one_step() {
        let directory = scratch("overwrite");
        let mut preset = preset_named("look-one", "First");
        save_at(&directory, &preset).unwrap();
        preset.name = "Second".to_owned();
        save_at(&directory, &preset).unwrap();

        let loaded = load_at(&directory, "look-one").expect("the preset reloads");
        assert_eq!(loaded.name, "Second");
        assert_eq!(loaded, preset.sanitized());

        let leftovers: Vec<String> = fs::read_dir(&directory)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files are cleaned up");
        assert_eq!(list_at(&directory).len(), 1);
    }

    #[test]
    fn an_updated_preset_keeps_the_id_and_name_it_replaces() {
        let directory = scratch("update");
        let original = preset_named("look-one", "Look one");
        save_at(&directory, &original).unwrap();

        let mut source = state();
        source.cursor = tuned_cursor();
        let captured = from_state(&source, "Captured").unwrap();
        save_at(&directory, &replacing(&original, captured)).unwrap();

        let loaded = load_at(&directory, "look-one").expect("the preset reloads");
        assert_eq!(loaded.name, "Look one");
        assert_eq!(loaded.cursor.settings(), tuned_cursor());
        assert_eq!(list_at(&directory).len(), 1, "the update replaces the file");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_preset_is_deleted_only_from_the_preset_directory() {
        let directory = scratch("delete");
        let escape = directory
            .parent()
            .expect("a scratch directory has a parent")
            .join("escape-delete.json");
        fs::write(&escape, b"not a preset").unwrap();

        for id in ["../escape-delete", "..\\escape-delete", "a/b", "", "."] {
            assert!(delete_at(&directory, id).is_err(), "id {id:?} is refused");
        }
        assert!(escape.is_file(), "nothing outside the directory is deleted");

        save_at(&directory, &preset_named("look-one", "Look")).unwrap();
        delete_at(&directory, "look-one").unwrap();
        assert!(load_at(&directory, "look-one").is_none());
        assert!(
            delete_at(&directory, "look-one").is_err(),
            "a missing preset is an error"
        );

        let _ = fs::remove_file(&escape);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_saved_preset_states_its_version_and_its_trimmed_name() {
        let directory = scratch("version");
        let mut preset = preset_named("look-one", "  Look one  ");
        save_at(&directory, &preset).unwrap();

        let text = fs::read_to_string(directory.join("look-one.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["version"], serde_json::json!(PRESET_VERSION));
        assert_eq!(value["id"], serde_json::json!("look-one"));
        assert_eq!(value["name"], serde_json::json!("Look one"));
        assert_eq!(value["camera"], serde_json::Value::Null);

        preset.version = PRESET_VERSION + 1;
        assert!(save_at(&directory, &preset).is_err());
    }
}
