#[test]
fn deb_package_includes_capture_helper_binary() {
    let cargo_toml = include_str!("../Cargo.toml");
    let workflow = include_str!("../.github/workflows/release.yml");
    let release_section = workflow
        .split("  release:\n")
        .nth(1)
        .expect("workflow should contain a release job");

    assert!(
        cargo_toml.contains("[\"packaging/deb/apexshot-capture\", \"usr/bin/\", \"755\"]"),
        "release .deb must include apexshot-capture in package.metadata.deb.assets"
    );

    assert!(
        cargo_toml.contains("depends = \"$auto"),
        "release .deb should rely on cargo-deb auto dependency detection for native runtime libraries"
    );

    assert!(
        release_section
            .contains("cp target/release/apexshot-capture packaging/deb/apexshot-capture"),
        "release workflow must stage apexshot-capture into packaging/deb before running cargo-deb"
    );

    assert!(
        release_section
            .contains("cmp target/release/apexshot-capture packaging/deb/apexshot-capture"),
        "release workflow must verify the staged apexshot-capture binary matches the fresh release build"
    );

    assert!(
        release_section.contains("cargo deb --no-build --verbose"),
        "release workflow must package the already-built binaries with cargo deb --no-build"
    );

    assert!(
        release_section.contains("- name: Build release binaries")
            && release_section.contains("cargo build --release --verbose"),
        "release workflow must build release binaries before staging apexshot-capture"
    );

    assert!(
        release_section.contains("image: ubuntu:25.10") || release_section.contains("runs-on: blacksmith-4vcpu-ubuntu-2404"),
        "release workflow must build release artifacts in Ubuntu 24.04/25.10 to match the target OCR ABI"
    );

    let uses_container = release_section.contains("image: ubuntu:25.10");
    if uses_container {
        assert!(
            release_section.contains("- name: Bootstrap container tooling")
                && release_section.contains("apt-get install -y curl ca-certificates git"),
            "release workflow container must install curl, certificates, and git before invoking the Rust toolchain action"
        );

        assert!(
            release_section.contains("apt-get update")
                && release_section.contains("apt-get install -y")
                && !release_section.contains("sudo apt-get update"),
            "containerized release job should install packages without sudo"
        );

        assert!(
            release_section.contains("ninja -C build install")
                && release_section.contains("ldconfig")
                && !release_section.contains("sudo ninja -C build install"),
            "containerized release job should install gtk4-layer-shell without sudo"
        );
    }

    assert!(
        release_section.contains("clang")
            && release_section.contains("cmake")
            && release_section.contains("libclang-dev"),
        "release job should install clang, cmake, and libclang-dev for native helper and bindgen build scripts"
    );
}

#[test]
fn build_script_tracks_all_capture_overlay_sources() {
    let build_script = include_str!("../build.rs");
    let cmake = include_str!("../capture-overlay/CMakeLists.txt");

    for line in cmake.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("src/") {
            continue;
        }

        let path = format!("capture-overlay/{}", trimmed);
        let needle = format!("println!(\"cargo:rerun-if-changed={}\")", path);
        assert!(
            build_script.contains(&needle),
            "build.rs must watch {} so cargo rebuilds apexshot-capture when that C++ file changes",
            path
        );
    }
}

#[test]
fn deb_package_includes_background_gradient_assets() {
    let cargo_toml = include_str!("../Cargo.toml");

    assert!(
        cargo_toml.contains("src/capture/editor/background-images/gradient-01.jpg")
            && cargo_toml.contains("src/capture/editor/background-images/gradient-10.jpg"),
        "release .deb must include the background gradient image assets in package.metadata.deb.assets"
    );

    assert!(
        cargo_toml.contains("usr/share/apexshot/background-images/"),
        "background gradient assets should be installed into a shared runtime directory"
    );
}
