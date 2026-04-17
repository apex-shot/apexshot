#[test]
fn deb_package_includes_capture_helper_binary() {
    let cargo_toml = include_str!("../Cargo.toml");
    let workflow = include_str!("../.github/workflows/release.yml");

    assert!(
        cargo_toml.contains("[\"packaging/deb/apexshot-capture\", \"usr/bin/\", \"755\"]"),
        "release .deb must include apexshot-capture in package.metadata.deb.assets"
    );

    assert!(
        cargo_toml.contains("depends = \"$auto\""),
        "release .deb should rely on cargo-deb auto dependency detection for native runtime libraries"
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

    assert!(
        workflow.contains("image: ubuntu:25.10"),
        "release workflow must build release artifacts in an Ubuntu 25.10 container to match the target OCR ABI"
    );

    assert!(
        workflow.contains("- name: Bootstrap container tooling")
            && workflow.contains("apt-get install -y curl ca-certificates git"),
        "release workflow container must install curl, certificates, and git before invoking the Rust toolchain action"
    );
}
