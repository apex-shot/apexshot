fn main() {
    println!("cargo:rerun-if-changed=../data/icons");
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed=APEXSHOT_STUDIO_VERIFY_KEY");
    println!("cargo:rerun-if-env-changed=APEXSHOT_STUDIO_RELEASE_TIMESTAMP");
    println!("cargo:rerun-if-env-changed=APEXSHOT_ALLOW_NO_STUDIO");
    configure_studio();
    configure_windows_resources();

    // The settings window reuses the editor's icon bundle so its dropdown
    // chevrons and custom page icons match the Linux build. The image editor
    // adds the annotation tools on top of the same bundle.
    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        Some("../data/icons"),
        [
            "chevron-down-regular",
            "search-regular",
            "chevron-right-regular",
            "chevron-up-regular",
            "pointer-primary-click",
            // The clip menu's rows use the same fluent glyphs as the Linux
            // editor, so a right-click reads the same on both platforms.
            "copy-regular",
            "copy-arrow-right-regular",
            "cut-regular",
            "clipboard-paste-regular",
            "eye-regular",
            "eye-off-regular",
            // Image editor: the one fluent glyph the tool rail keeps; every
            // other tool icon comes from the bundled symbolic set.
            "number-circle-1-regular",
            // The Appearance panel's Image tab trails the Linux chooser's
            // folder affordance.
            "folder-open-regular",
            // The Motion notch borrows the watermark tab's photo glyph.
            "image-regular",
            // Cloud sign-in: the back pill and the link's open glyph.
            "chevron-left-regular",
            "open-regular",
            // The Clips panel's rail icon and the glyphs its sound controls
            // name: the clip itself, its two tracks, music and speed.
            "video-clip-regular",
            "speaker-2-regular",
            "speaker-mute-regular",
            "mic-regular",
            "music-note-2-regular",
            "dismiss-regular",
            "top-speed-regular",
        ],
    );
}

fn configure_studio() {
    const KEY: &str = "APEXSHOT_STUDIO_VERIFY_KEY";
    const TIMESTAMP: &str = "APEXSHOT_STUDIO_RELEASE_TIMESTAMP";
    let env_key = std::env::var(KEY).unwrap_or_default();
    let env_timestamp = std::env::var(TIMESTAMP).unwrap_or_default();
    let (key, timestamp) = if !env_key.is_empty() || !env_timestamp.is_empty() {
        (env_key, env_timestamp)
    } else {
        let mut key = String::new();
        let mut timestamp = String::new();
        if let Ok(contents) = std::fs::read_to_string("../.env") {
            for line in contents.lines().map(str::trim) {
                if let Some((name, value)) = line.split_once('=') {
                    let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                    match name.trim() {
                        KEY => key = value.to_owned(),
                        TIMESTAMP => timestamp = value.to_owned(),
                        _ => {}
                    }
                }
            }
        }
        (key, timestamp)
    };
    if key.is_empty() && timestamp.is_empty() {
        assert!(
            std::env::var("PROFILE").as_deref() != Ok("release")
                || std::env::var("APEXSHOT_ALLOW_NO_STUDIO").as_deref() == Ok("1"),
            "Studio release builds require APEXSHOT_STUDIO_VERIFY_KEY and APEXSHOT_STUDIO_RELEASE_TIMESTAMP; set APEXSHOT_ALLOW_NO_STUDIO=1 to opt out"
        );
        println!(
            "cargo:warning=Studio verification is unavailable: build configuration is missing"
        );
        return;
    }
    assert!(
        key.len() == 43
            && key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            && timestamp.parse::<i64>().is_ok_and(|value| value > 0),
        "Invalid Studio verification key or release timestamp"
    );
    println!("cargo:rustc-env={KEY}={key}");
    println!("cargo:rustc-env={TIMESTAMP}={timestamp}");
}

/// Give every Windows binary the app icon and the version block, so Explorer,
/// the taskbar, the Start-menu shortcut, and Add/Remove Programs show ApexShot
/// instead of a blank executable.
///
/// The resource only exists for Windows targets, and only a Windows host has an
/// `rc.exe` to compile it, so every other case builds unchanged.
#[cfg(windows)]
fn configure_windows_resources() {
    if !std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        return;
    }

    println!("cargo:rerun-if-changed=../packaging/windows/apexshot.rc");
    println!("cargo:rerun-if-changed=../packaging/windows/apexshot.ico");

    // `FILEVERSION` wants four comma-separated numbers; `FileVersion` wants the
    // version as it is spelled in Cargo.toml.
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0));
    let numbers = format!(
        "{},{},{},0",
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0)
    );

    let compiled = embed_resource::compile(
        "../packaging/windows/apexshot.rc",
        embed_resource::ParamsMacrosAndIncludeDirs(
            [
                format!("VERSION={numbers}"),
                // rc substitutes the value verbatim, so the quotes belong in
                // the macro: `VALUE "FileVersion", VERSION_STR` has to expand
                // to a quoted string, not to an escaped one.
                format!("VERSION_STR=\"{version}\""),
            ],
            ["../packaging/windows"],
        ),
    );
    if compiled != embed_resource::CompilationResult::Ok {
        println!("cargo:warning=the Windows icon and version block were not embedded: {compiled}");
    }
}

#[cfg(not(windows))]
fn configure_windows_resources() {}
