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
        release_section.contains("image: ubuntu:25.10"),
        "release workflow must build release artifacts in an Ubuntu 25.10 container to match the target OCR ABI"
    );

    assert!(
        release_section.contains("- name: Bootstrap container tooling")
            && release_section.contains("curl ca-certificates git"),
        "release workflow container must install curl, certificates, and git before invoking the Rust toolchain action"
    );

    assert!(
        release_section.contains("apt-get update")
            && release_section.contains("apt-get install -y")
            && !release_section.contains("sudo apt-get update"),
        "containerized release job should install packages without sudo"
    );

    assert!(
        release_section.contains("clang")
            && release_section.contains("cmake")
            && release_section.contains("libclang-dev"),
        "containerized release job should install clang, cmake, and libclang-dev for native helper and bindgen build scripts"
    );

    assert!(
        release_section.contains("ninja -C build install")
            && release_section.contains("ldconfig")
            && !release_section.contains("sudo ninja -C build install"),
        "containerized release job should install gtk4-layer-shell without sudo"
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

#[test]
fn arch_pkgbuild_version_matches_cargo_package_version() {
    let cargo_toml = include_str!("../Cargo.toml");
    let pkgbuild = include_str!("../packaging/arch/PKGBUILD");

    let cargo_version = cargo_toml
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("Cargo.toml should declare package version");
    let pkgver = pkgbuild
        .lines()
        .find_map(|line| line.trim().strip_prefix("pkgver="))
        .expect("PKGBUILD should declare pkgver");

    assert_eq!(
        pkgver, cargo_version,
        "Arch PKGBUILD pkgver must match Cargo.toml package version"
    );

    let expected_source = format!("archive/v{cargo_version}.tar.gz");
    assert!(
        pkgbuild.contains(&expected_source),
        "Arch PKGBUILD source should download the matching release tag"
    );
}

#[test]
fn opensuse_installer_contains_reported_dependency_set() {
    let install_script = include_str!("../scripts/opensuse-install.sh");
    let update_script = include_str!("../scripts/opensuse-update.sh");
    let generic_install = include_str!("../scripts/install.sh");
    let generic_update = include_str!("../scripts/update.sh");

    for package in [
        "curl",
        "ffmpeg",
        "gstreamer-plugins-base",
        "gstreamer-plugins-good",
        "gstreamer-plugins-bad",
        "gstreamer-plugin-pipewire",
        "pipewire",
        "pipewire-pulseaudio",
        "tesseract-ocr",
        "unzip",
        "wget",
        "wl-clipboard",
        "xdg-desktop-portal",
        "xdg-utils",
        "update-desktop-files",
    ] {
        assert!(
            install_script.contains(package),
            "openSUSE installer should include dependency {package}"
        );
    }

    assert!(
        install_script.contains("zypper --non-interactive install --needed"),
        "openSUSE installer should install dependencies through zypper"
    );
    assert!(
        install_script.contains("resolve_rpm_url") && install_script.contains("download_rpm"),
        "openSUSE installer should resolve and download the published RPM"
    );
    assert!(
        install_script.contains("zypper --non-interactive install '${RPM_FILE}'"),
        "openSUSE installer should install the downloaded RPM with zypper"
    );
    assert!(
        update_script.contains("opensuse-install.sh") && update_script.contains("--force"),
        "openSUSE updater should refresh the RPM install through the installer"
    );
    assert!(
        generic_install.contains("command -v zypper")
            && generic_install.contains("opensuse-install.sh"),
        "generic installer should dispatch to the openSUSE installer"
    );
    assert!(
        generic_update.contains("command -v zypper")
            && generic_update.contains("opensuse-update.sh"),
        "generic updater should dispatch to the openSUSE updater"
    );
}

#[test]
fn opensuse_rpm_spec_matches_project_packaging_contract() {
    let cargo_toml = include_str!("../Cargo.toml");
    let spec = include_str!("../packaging/opensuse/apexshot.spec");
    let build_script = include_str!("../scripts/build-opensuse-rpm.sh");
    let main_rs = include_str!("../src/main.rs");

    let cargo_version = cargo_toml
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("Cargo.toml should declare package version");
    let spec_version = spec
        .lines()
        .find_map(|line| line.trim().strip_prefix("Version:"))
        .map(str::trim)
        .expect("openSUSE spec should declare Version");

    assert_eq!(
        spec_version, cargo_version,
        "openSUSE RPM spec Version must match Cargo.toml package version"
    );

    for package in [
        "gtk4-devel",
        "gtk4-layer-shell-devel",
        "libadwaita-devel",
        "libQt5Core-devel",
        "libqt5-qtx11extras-devel",
        "pipewire-devel",
        "tesseract-ocr-devel",
        "gstreamer-plugin-pipewire",
        "xdg-desktop-portal",
        "wl-clipboard",
        "ffmpeg",
    ] {
        assert!(
            spec.contains(package),
            "openSUSE RPM spec should include package {package}"
        );
    }

    for payload in [
        "%{_bindir}/apexshot",
        "%{_bindir}/apexshot-capture",
        "%{_bindir}/apexshot-native-host",
        "%{_datadir}/applications/io.github.codegoddy.apexshot.desktop",
        "%{_datadir}/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/",
        "%{_datadir}/apexshot/",
        "%{_sysconfdir}/opt/chrome/NativeMessagingHosts/io.github.codegoddy.apexshot.json",
        "%{_sysconfdir}/chromium/NativeMessagingHosts/io.github.codegoddy.apexshot.json",
    ] {
        assert!(
            spec.contains(payload),
            "openSUSE RPM spec should package {payload}"
        );
    }

    assert!(
        build_script.contains("git -C \"$REPO_DIR\" archive")
            && build_script.contains("rpmbuild")
            && build_script.contains("_topdir ${RPM_TOPDIR}"),
        "openSUSE RPM build helper should create a source archive and call rpmbuild with a local topdir"
    );

    assert!(
        main_rs.contains("rpm_has_apexshot_package")
            && main_rs.contains("\"zypper\"")
            && main_rs.contains("\"--non-interactive\"")
            && main_rs.contains("\"remove\""),
        "RPM package-managed installs should uninstall through zypper"
    );
}
