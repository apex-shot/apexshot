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
        release_section.contains("scripts/package-deb.sh --no-build --verbose"),
        "release workflow must package the already-built binaries with cargo deb --no-build"
    );

    assert!(
        release_section.contains("- name: Build release binaries")
            && release_section.contains("cargo build --release --verbose"),
        "release workflow must build release binaries before staging apexshot-capture"
    );

    assert!(
        release_section.contains("image: ubuntu:24.04"),
        "release workflow must build release artifacts in an Ubuntu 24.04 container to match the target OCR ABI"
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
fn deb_package_bundles_layer_shell_in_a_private_runtime_directory() {
    let cargo_toml = include_str!("../Cargo.toml");
    let build_script = include_str!("../build.rs");
    let packaging_script = include_str!("../scripts/package-deb.sh");

    assert!(
        cargo_toml.contains("target/deb-staging/libgtk4-layer-shell.so.0\", \"usr/lib/apexshot/"),
        "the .deb must ship gtk4-layer-shell outside distro-owned multiarch paths"
    );
    assert!(
        cargo_toml.contains("libgtk-4-1"),
        "the .deb must declare GTK4 explicitly when the private layer-shell library bypasses shlib metadata"
    );
    assert!(
        cargo_toml.contains("gtk4-layer-shell.MIT-LICENSE")
            && std::path::Path::new("packaging/debian/gtk4-layer-shell.MIT-LICENSE").exists(),
        "the private runtime copy must include its upstream MIT license"
    );
    assert!(
        build_script.contains("-Wl,-rpath,$ORIGIN/../lib/apexshot"),
        "the ApexShot executable must resolve its privately bundled layer-shell library"
    );
    assert!(
        packaging_script.contains("cp -L \"$library\" \"$staged_library\"")
            && packaging_script.contains("cargo deb \"$@\""),
        "the Debian packaging entrypoint must stage the installed library before cargo-deb"
    );
}

#[test]
fn gnome_extension_metadata_declares_gnome_46_without_dropping_newer_shells() {
    let metadata: serde_json::Value =
        serde_json::from_str(include_str!("../gnome-extension/metadata.json"))
            .expect("extension metadata must be valid JSON");
    let workflow = include_str!("../.github/workflows/release.yml");
    let release_section = workflow
        .split("  release:\n")
        .nth(1)
        .expect("workflow should contain a release job");
    let versions = metadata["shell-version"]
        .as_array()
        .expect("shell-version must be an array");

    for supported in ["46", "48", "49", "50"] {
        assert!(
            versions.iter().any(|version| version == supported),
            "GNOME Shell {supported} must remain declared as supported"
        );
    }

    assert!(
        !versions.iter().any(|version| version == "45"),
        "GNOME 46 support must not broaden to GNOME 45"
    );
    assert!(
        release_section.contains("zip apexshot-gnome-integration.zip")
            && release_section.contains("extension.js metadata.json"),
        "the release ZIP must ship the same declared shell compatibility"
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

/// Every `.js` module beside `shell-overlay.js` is part of the shipped
/// extension and must reach the package.
///
/// The names are read from the directory instead of being written out here: a
/// hand-maintained list is exactly how `press-tracker.js` was left out of every
/// package while these tests stayed green.
fn gnome_extension_js_files() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("gnome-extension");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("gnome-extension directory must exist")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.ends_with(".js"))
        .collect();
    files.sort();
    assert!(
        files.len() > 1,
        "expected the GNOME extension modules in {dir:?}, found {files:?}"
    );
    files
}

#[test]
fn deb_package_includes_gnome_extension() {
    let cargo_toml = include_str!("../Cargo.toml");
    for file in gnome_extension_js_files() {
        let asset = format!("gnome-extension/{file}");
        assert!(
            cargo_toml.contains(&asset),
            "release .deb must include {asset}"
        );
    }
    assert!(
        cargo_toml.contains("gnome-extension/metadata.json"),
        "release .deb must include the extension metadata"
    );
    assert!(
        cargo_toml.contains(
            "usr/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/"
        ),
        "GNOME extension assets must be installed in the system extension directory"
    );
}

#[test]
fn dev_deb_reinstall_refreshes_gnome_extension() {
    let reinstall_script = include_str!("../reinstall-dev-deb.sh");
    let mut expected = vec!["metadata.json".to_string()];
    expected.extend(gnome_extension_js_files());
    for file in expected {
        assert!(
            reinstall_script.contains(file.as_str()),
            "development .deb reinstall must refresh GNOME extension file {file}"
        );
    }

    assert!(
        reinstall_script.contains("mktemp")
            && reinstall_script.contains("install -m 0644 \"$newest_deb\""),
        "development .deb reinstall must stage a package readable by APT's sandbox"
    );
    assert!(
        reinstall_script.contains("cmp \"$ROOT_DIR/gnome-extension/$file\"")
            && reinstall_script.contains("$SYSTEM_EXT/$file"),
        "development .deb reinstall must verify the packaged system extension"
    );
    assert!(
        !reinstall_script.contains("extension_changed"),
        "development .deb reinstall must always refresh and reload the live user extension"
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
        install_script.contains("zypper --non-interactive --no-gpg-checks install '${RPM_FILE}'"),
        "openSUSE installer should install the downloaded RPM with zypper"
    );
    assert!(
        install_script.contains("zypper --non-interactive --no-gpg-checks install --force '${RPM_FILE}'"),
        "openSUSE installer should reinstall the RPM through zypper when forced"
    );
    assert!(
        update_script.contains("opensuse-install.sh") && update_script.contains("--force"),
        "openSUSE updater should refresh the RPM install through the installer"
    );
    assert!(
        generic_install.contains("command -v zypper")
            && generic_install.contains("opensuse")
            && generic_install.contains("build-opensuse-rpm.sh"),
        "generic installer should detect openSUSE via zypper and point at the local RPM build path"
    );
    assert!(
        generic_update.contains("command -v zypper")
            && generic_update.contains("opensuse")
            && generic_update.contains("build-opensuse-rpm.sh"),
        "generic updater should detect openSUSE via zypper and point at the local RPM build path"
    );
}

#[test]
fn opensuse_rpm_spec_matches_project_packaging_contract() {
    let cargo_toml = include_str!("../Cargo.toml");
    let spec = include_str!("../packaging/opensuse/apexshot.spec");
    let build_script = include_str!("../scripts/build-opensuse-rpm.sh");
    let install_rs = include_str!("../src/cli/install.rs");

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
        install_rs.contains("rpm_has_apexshot_package")
            && install_rs.contains("\"zypper\"")
            && install_rs.contains("\"--non-interactive\"")
            && install_rs.contains("\"remove\""),
        "RPM package-managed installs should uninstall through zypper"
    );
}
