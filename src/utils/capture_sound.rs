//! Configured shutter-sound playback for the native capture freeze path.

use std::path::PathBuf;

use crate::config::load_config;

fn should_play_shutter_sound(play_sounds: bool, shutter_sound: &str) -> bool {
    play_sounds && shutter_sound != "None"
}

fn shutter_sound_file_name(sound_name: &str) -> Option<&'static str> {
    match sound_name {
        "Camera" => Some("camera.ogg"),
        "Classic" => Some("classic.ogg"),
        "Pop" => Some("pop.ogg"),
        _ => None,
    }
}

fn shutter_sound_asset_path(sound_name: &str) -> Option<PathBuf> {
    let file_name = shutter_sound_file_name(sound_name)?;
    [
        // Development: relative to the current project directory.
        std::env::current_dir()
            .unwrap_or_default()
            .join("assets/sounds")
            .join(file_name),
        // Installed: relative to binary location
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("assets/sounds").join(file_name))
            })
            .unwrap_or_default(),
        // System-wide install
        PathBuf::from("/usr/share/apexshot/sounds").join(file_name),
        PathBuf::from("/usr/local/share/apexshot/sounds").join(file_name),
    ]
    .into_iter()
    .find(|path| !path.as_os_str().is_empty() && path.exists())
}

/// Play the configured shutter sound once, for the initial freeze of a native
/// capture. Portal routes leave capture audio to the desktop and never call it.
pub(crate) fn play_shutter_sound_if_enabled() {
    let config = load_config().sanitized();
    if !should_play_shutter_sound(config.play_sounds, &config.shutter_sound) {
        return;
    }

    let Some(sound_path) = shutter_sound_asset_path(&config.shutter_sound) else {
        eprintln!(
            "[capture] Shutter sound '{}' selected but asset file is not available yet",
            config.shutter_sound
        );
        return;
    };

    let playback = std::process::Command::new("sh")
        .arg("-c")
        .arg(
            "if command -v pw-play >/dev/null 2>&1; then pw-play \"$1\"; \
             elif command -v paplay >/dev/null 2>&1; then paplay \"$1\"; \
             elif command -v aplay >/dev/null 2>&1; then aplay \"$1\"; \
             else exit 127; fi",
        )
        .arg("sh")
        .arg(&sound_path)
        .spawn();

    if let Err(e) = playback {
        eprintln!(
            "[capture] Failed to start shutter sound playback for {}: {e}",
            sound_path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_sounds_never_play() {
        assert!(!should_play_shutter_sound(false, "Camera"));
    }

    #[test]
    fn none_selection_never_plays() {
        assert!(!should_play_shutter_sound(true, "None"));
    }

    #[test]
    fn enabled_named_selection_plays() {
        assert!(should_play_shutter_sound(true, "Camera"));
        assert!(should_play_shutter_sound(true, "Classic"));
        assert!(should_play_shutter_sound(true, "Pop"));
    }

    #[test]
    fn none_selection_has_no_asset() {
        assert!(shutter_sound_asset_path("None").is_none());
    }

    #[test]
    fn unknown_selection_has_no_asset() {
        assert!(shutter_sound_asset_path("Chime").is_none());
    }

    #[test]
    fn bundled_selection_resolves_from_the_checkout() {
        let path = shutter_sound_asset_path("Camera").expect("bundled sound should resolve");
        assert!(path.ends_with("assets/sounds/camera.ogg"));
    }
}
