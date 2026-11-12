//! The look a recording opens on the first time it becomes a project.
//!
//! A new project does not start on a flat video: it carries one bundled
//! wallpaper and a wider inset than the panel's own starting padding, so the
//! first paint already looks like a finished edit. A configured default preset
//! replaces that bundled look with the one the user saved, and a recording that
//! captured a region or a window then opens with rounded corners sized to what
//! it recorded; a full-screen recording opens square, because the corners are a
//! record of the capture and outrank a saved look. Only a project that has just
//! been created is seeded; a saved project keeps whatever it stored.

use super::presets::{self, Preset};
use crate::recording::editor::sidecar::PointerSidecar;
use std::path::{Path, PathBuf};

/// The inset a fresh import opens with, in the Background panel's padding
/// units, which are expressed against a 400px long edge.
pub(super) const IMPORT_PADDING: f64 = 40.0;

/// The corner radius a captured region or window opens with, in pixels at the
/// size that was recorded.
const IMPORT_CORNER_RADIUS_PX: f64 = 16.0;

/// Put the opening look on a project file that has just been created.
pub(super) fn seed(file: &mut super::project::EditorProjectFile) {
    seed_with_preset(file, configured_preset().as_ref());
}

/// The preset the user chose as every recording's opening look.
///
/// Read only while a project is being created, never while one is open, so a
/// preset saved afterwards cannot change a recording already being edited. No
/// configured id, or a preset that has been moved, deleted or corrupted, reads
/// as none, which keeps the bundled look rather than failing the import.
fn configured_preset() -> Option<Preset> {
    crate::config::load_config()
        .rec_video_default_preset
        .as_deref()
        .and_then(presets::load)
}

/// The opening look as a preset this call was handed: the bundled stage first,
/// the preset over it, then the corners the recording's own manifest asks for.
///
/// `seed` is this with the preset the user configured; naming the preset here
/// is also what lets a test open a project without touching that config.
pub(super) fn seed_with_preset(
    file: &mut super::project::EditorProjectFile,
    preset: Option<&Preset>,
) {
    file.background.selection = Some("wallpaper-004.jpg".to_owned());
    file.background_padding = IMPORT_PADDING;
    if let Some(preset) = preset {
        presets::apply_to(file, preset);
    }
    file.background_radius = manifest_radius(&file.source_path);
}

fn manifest_radius(source: &Path) -> f64 {
    let mut path = PointerSidecar::resolved_sidecar_path(source).into_os_string();
    path.push(".recording.json");
    std::fs::read_to_string(PathBuf::from(path))
        .ok()
        .as_deref()
        .map(radius_from_manifest)
        .unwrap_or(0.0)
}

/// The corner radius a recording's manifest asks for, in the Background panel's
/// radius units, which are expressed against a 400px long edge.
///
/// Only a region or window capture gets corners: its recorded output is smaller
/// than the screen, so the frame around it is what the corners are for. The
/// radius is a fixed pixel size at the recorded output, so a small capture gets
/// more units than a large one. A screen capture, an unknown target, a manifest
/// that cannot be read and one without usable output dimensions all open square.
fn radius_from_manifest(text: &str) -> f64 {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(text) else {
        return 0.0;
    };
    if !matches!(
        manifest.get("target").and_then(|target| target.as_str()),
        Some("area" | "window")
    ) {
        return 0.0;
    }
    let output = manifest.get("output");
    let dimension = |key: &str| {
        output
            .and_then(|output| output.get(key))
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value >= 2)
    };
    let Some((width, height)) = dimension("width").zip(dimension("height")) else {
        return 0.0;
    };
    let long_edge = f64::from(width.max(height));
    IMPORT_CORNER_RADIUS_PX * super::background::PADDING_REFERENCE / long_edge
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::{ClickEffect, CursorSettings, CursorTheme};

    /// The smallest project a fresh recording produces: a source, its probed
    /// length, different trim points, zoom settings and a camera layer that is
    /// not the default one.
    fn project() -> super::super::project::EditorProjectFile {
        serde_json::from_value(serde_json::json!({
            "version": 3,
            "source_path": "C:\\Videos\\recording.mp4",
            "source_size": 4096,
            "source_mtime_secs": 1_700_000_000,
            "trim_start_seconds": 1.0,
            "trim_end_seconds": 9.5,
            "freeze_tail": 0.5,
            "zoom_camera": {
                "speed": 1.5,
                "smoothness": 0.25,
                "start_early": true,
                "motion_blur": 0.7,
            },
            "webcam": {
                "enabled": true,
                "fullscreen": false,
                "x": 0.62,
                "y": 0.31,
                "w": 0.18,
                "h": 0.24,
                "radius": 0.5,
                "mirror": true,
                "delay_seconds": 0.04,
            },
        }))
        .expect("a minimal project reads")
    }

    fn manifest(target: &str, width: u64, height: u64) -> String {
        serde_json::json!({
            "version": 1,
            "target": target,
            "output": { "width": width, "height": height },
        })
        .to_string()
    }

    fn manifest_with_output(output: serde_json::Value) -> String {
        serde_json::json!({
            "version": 1,
            "target": "window",
            "output": output,
        })
        .to_string()
    }

    fn radius_units(long_edge: f64) -> f64 {
        IMPORT_CORNER_RADIUS_PX * super::super::background::PADDING_REFERENCE / long_edge
    }

    #[test]
    fn a_new_project_opens_on_the_import_stage() {
        let file = project();
        let mut seeded = file.clone();
        seed_with_preset(&mut seeded, None);

        assert_eq!(
            seeded.background.selection.as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert_eq!(seeded.background_padding, 40.0);
        assert_eq!(seeded.background_radius, 0.0);

        let mut expected = file.clone();
        expected.background.selection = Some("wallpaper-004.jpg".to_owned());
        expected.background_padding = IMPORT_PADDING;
        expected.background_radius = 0.0;
        assert_eq!(seeded, expected);
        assert_eq!(seeded.trim_start_seconds, 1.0);
        assert_eq!(seeded.trim_end_seconds, 9.5);
    }

    #[test]
    fn a_window_or_area_capture_opens_with_corners() {
        for target in ["window", "area"] {
            let landscape = radius_from_manifest(&manifest(target, 1920, 1080));
            let portrait = radius_from_manifest(&manifest(target, 1080, 1920));
            assert!((landscape - radius_units(1920.0)).abs() < 1e-9);
            assert!((portrait - radius_units(1920.0)).abs() < 1e-9);
        }
    }

    #[test]
    fn an_encoded_resolution_gets_its_own_corner_units() {
        let radius = radius_from_manifest(&manifest("window", 1280, 720));
        assert!((radius - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_screen_capture_opens_square() {
        assert_eq!(radius_from_manifest(&manifest("screen", 1920, 1080)), 0.0);
    }

    #[test]
    fn an_unreadable_manifest_opens_square() {
        assert_eq!(radius_from_manifest("{not json"), 0.0);
        assert_eq!(radius_from_manifest(""), 0.0);
        assert_eq!(radius_from_manifest("null"), 0.0);
    }

    #[test]
    fn an_unknown_or_missing_target_opens_square() {
        assert_eq!(radius_from_manifest(&manifest("camera", 1920, 1080)), 0.0);
        let mut value: serde_json::Value =
            serde_json::from_str(&manifest("window", 1920, 1080)).unwrap();
        value.as_object_mut().unwrap().remove("target");
        assert_eq!(radius_from_manifest(&value.to_string()), 0.0);
    }

    #[test]
    fn a_manifest_without_usable_output_opens_square() {
        let mut missing: serde_json::Value =
            serde_json::from_str(&manifest("window", 1920, 1080)).unwrap();
        missing.as_object_mut().unwrap().remove("output");
        assert_eq!(radius_from_manifest(&missing.to_string()), 0.0);
        assert_eq!(radius_from_manifest(&manifest("window", 1, 1080)), 0.0);
        assert_eq!(radius_from_manifest(&manifest("window", 1920, 0)), 0.0);

        let null = manifest_with_output(serde_json::Value::Null);
        assert_eq!(radius_from_manifest(&null), 0.0);

        let text_size = serde_json::json!({ "width": "1920", "height": "1080" });
        assert_eq!(radius_from_manifest(&manifest_with_output(text_size)), 0.0);

        let fractional = serde_json::json!({ "width": 1920.5, "height": 1080 });
        assert_eq!(radius_from_manifest(&manifest_with_output(fractional)), 0.0);
    }

    #[test]
    fn a_seeded_area_capture_takes_its_corners_from_the_manifest() {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-windows-import-seed-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("recording.mp4");
        std::fs::write(&source, b"not really a video").unwrap();

        let mut manifest_path = PointerSidecar::resolved_sidecar_path(&source).into_os_string();
        manifest_path.push(".recording.json");
        let manifest_path = PathBuf::from(manifest_path);
        if let Some(parent) = manifest_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&manifest_path, manifest("area", 1920, 1080)).unwrap();

        let mut file = project();
        file.source_path = source;
        let mut expected = file.clone();
        seed_with_preset(&mut file, None);

        assert_eq!(
            file.background.selection.as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert_eq!(file.background_padding, IMPORT_PADDING);
        assert!((file.background_radius - radius_units(1920.0)).abs() < 1e-9);

        expected.background.selection = Some("wallpaper-004.jpg".to_owned());
        expected.background_padding = IMPORT_PADDING;
        expected.background_radius = radius_units(1920.0);
        assert_eq!(file, expected);

        let _ = std::fs::remove_file(&manifest_path);
        let _ = std::fs::remove_dir_all(&directory);
    }

    fn cursor() -> CursorSettings {
        CursorSettings {
            theme: CursorTheme::White,
            size: 1.5,
            click_effect: ClickEffect::Circle,
            ..CursorSettings::default()
        }
    }

    /// A saved look as the preset store would hand it back: a wider inset than
    /// the bundled one, its own cursor and the effects that come with it.
    fn preset(padding: f64) -> super::super::presets::Preset {
        let mut source = project();
        source.background = super::super::project::StageFile {
            selection: Some("wallpaper-019.jpg".to_owned()),
            aspect: None,
        };
        source.background_padding = padding;
        source.background_radius = 9.0;
        source.captions_shortcuts = true;
        source.cursor_click_sound = Some("soft".to_owned());
        source.cursor_click_sound_volume = 0.4;
        super::super::presets::from_project(&source, cursor(), "Opening look")
            .expect("a preset captures")
    }

    #[test]
    fn a_configured_preset_replaces_the_bundled_opening_look() {
        let look = preset(60.0);
        let mut seeded = project();
        seeded.webcam = None;
        seed_with_preset(&mut seeded, Some(&look));

        assert_eq!(seeded.background_padding, 60.0);
        assert_eq!(
            seeded.background.selection.as_deref(),
            Some("wallpaper-019.jpg"),
            "the preset's stage replaces the bundled one"
        );
        assert_eq!(seeded.cursor, look.cursor);
        assert!(seeded.captions_shortcuts);
        assert_eq!(seeded.cursor_click_sound.as_deref(), Some("soft"));
        assert!((seeded.cursor_click_sound_volume - 0.4).abs() < 1e-9);
        assert!(
            seeded.webcam.is_none(),
            "a preset never creates a camera layer"
        );
    }

    #[test]
    fn the_corners_a_recording_asks_for_win_over_the_preset() {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-windows-import-preset-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("recording.mp4");
        std::fs::write(&source, b"not really a video").unwrap();

        let mut manifest_path = PointerSidecar::resolved_sidecar_path(&source).into_os_string();
        manifest_path.push(".recording.json");
        let manifest_path = PathBuf::from(manifest_path);
        if let Some(parent) = manifest_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&manifest_path, manifest("area", 1920, 1080)).unwrap();

        let mut file = project();
        file.source_path = source;
        seed_with_preset(&mut file, Some(&preset(60.0)));

        assert_eq!(file.background_padding, 60.0);
        assert!((file.background_radius - radius_units(1920.0)).abs() < 1e-9);

        let _ = std::fs::remove_file(&manifest_path);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
