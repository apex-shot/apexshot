#[test]
fn packaged_desktop_identity_matches_primary_ui_application_id() {
    let cargo_toml = include_str!("../Cargo.toml");
    let capture_overlay_source = include_str!("../src/capture_overlay.rs");
    let settings_source = include_str!("../src/settings/mod.rs");
    let onboarding_source = include_str!("../src/onboarding/mod.rs");

    assert!(
        cargo_toml.contains(
            "[\"packaging/apexshot.desktop\", \"usr/share/applications/io.github.codegoddy.apexshot.desktop\", \"644\"]"
        ),
        "the packaged desktop entry should install under io.github.codegoddy.apexshot.desktop"
    );

    assert!(
        settings_source.contains(".application_id(\"io.github.codegoddy.apexshot\")"),
        "settings window should use the packaged desktop application id so docks show the ApexShot icon"
    );

    assert!(
        onboarding_source.contains(".application_id(\"io.github.codegoddy.apexshot\")"),
        "onboarding window should use the packaged desktop application id so docks show the ApexShot icon"
    );

    assert!(
        capture_overlay_source.contains("scoped_portal_capture_identity()"),
        "native capture subprocesses should override daemon desktop identity so portal grants persist under the main app id"
    );
}
