#[test]
fn packaged_desktop_identity_matches_primary_ui_application_id() {
    let cargo_toml = include_str!("../Cargo.toml");
    let capture_overlay_source = include_str!("../src/capture_overlay.rs");
    let settings_source = include_str!("../src/settings/mod.rs");
    let onboarding_source = include_str!("../src/onboarding/mod.rs");
    let packaged_desktop = include_str!("../packaging/apexshot.desktop");
    let packaged_daemon_desktop = include_str!("../packaging/apexshot-daemon.desktop");
    let main_source = include_str!("../src/main.rs");
    let windowing_source = include_str!("../src/settings/windowing.rs");

    assert!(
        cargo_toml.contains(
            "[\"packaging/apexshot.desktop\", \"usr/share/applications/io.github.codegoddy.apexshot.desktop\", \"644\"]"
        ),
        "the packaged desktop entry should install under io.github.codegoddy.apexshot.desktop"
    );

    assert!(
        settings_source.contains(".application_id(crate::app_identity::app_id())"),
        "settings window should use the packaged desktop application id so docks show the ApexShot icon"
    );

    assert!(
        onboarding_source.contains(".application_id(crate::app_identity::app_id())"),
        "onboarding window should use the packaged desktop application id so docks show the ApexShot icon"
    );

    assert!(
        capture_overlay_source.contains("scoped_portal_capture_identity()"),
        "native capture subprocesses should override daemon desktop identity so portal grants persist under the main app id"
    );

    assert!(
        packaged_desktop.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
        "the packaged desktop entry should declare KWin screenshot authorization for KDE Plasma Wayland"
    );

    assert!(
        packaged_daemon_desktop
            .contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
        "the packaged daemon desktop entry should declare KWin screenshot authorization for KDE Plasma Wayland"
    );

    assert!(
        main_source.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
        "dev-generated desktop entries should declare KWin screenshot authorization"
    );

    assert!(
        windowing_source.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"),
        "autostart desktop entries should preserve KWin screenshot authorization"
    );
}
