#[test]
fn deb_package_includes_capture_helper_binary() {
    let cargo_toml = include_str!("../Cargo.toml");
    let workflow = include_str!("../.github/workflows/release.yml");

    assert!(
        cargo_toml.contains("[\"packaging/deb/apexshot-capture\", \"usr/bin/\", \"755\"]"),
        "release .deb must include apexshot-capture in package.metadata.deb.assets"
    );

    assert!(
        workflow.contains("cp target/release/apexshot-capture packaging/deb/apexshot-capture"),
        "release workflow must stage apexshot-capture into packaging/deb before running cargo-deb"
    );

    assert!(
        workflow.contains("cargo deb --no-build --verbose"),
        "release workflow must package the already-built binaries with cargo deb --no-build"
    );

    assert!(
        workflow.contains("- name: Build release binaries")
            && workflow.contains("cargo build --release --verbose"),
        "release workflow must build release binaries before staging apexshot-capture"
    );
}
