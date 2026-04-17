#[test]
fn deb_package_includes_capture_helper_binary() {
    let cargo_toml = include_str!("../Cargo.toml");

    assert!(
        cargo_toml.contains("[\"target/release/apexshot-capture\", \"usr/bin/\", \"755\"]"),
        "release .deb must include apexshot-capture in package.metadata.deb.assets"
    );
}
