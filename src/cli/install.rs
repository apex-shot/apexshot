use apexshot::app_identity;

/// Install the binary to /usr/local/bin/ and set up autostart.
pub(crate) fn run_install(args: &[String]) {
    let mut no_autostart = false;
    let mut no_binary = false;
    let mut force = false;
    let mut dev_install = false;
    let mut extension_id: Option<String> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--no-autostart" => {
                no_autostart = true;
                i += 1;
            }
            "--no-binary" => {
                no_binary = true;
                i += 1;
            }
            "--force" => {
                force = true;
                i += 1;
            }
            "--dev" => {
                dev_install = true;
                i += 1;
            }
            "--extension-id" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --extension-id requires a value");
                    std::process::exit(1);
                }
                extension_id = Some(args[i + 1].clone());
                i += 2;
            }
            other => {
                eprintln!("Error: unknown install option '{other}'");
                std::process::exit(1);
            }
        }
    }

    if !no_binary {
        install_binary(force, dev_install);
        install_desktop_launcher(dev_install);
    }

    if !no_autostart {
        install_autostart(dev_install);
    }

    if let Some(id) = extension_id {
        if let Err(e) = super::native_host::install_native_host_manifest(
            &id,
            super::native_host::BrowserTarget::Both,
        ) {
            eprintln!("Error: failed to install native host: {e}");
            std::process::exit(1);
        }
    }

    // Persist XDG portal permissions so the user doesn't have to re-approve
    // screenshot/screencast access after every reboot.
    apexshot::backend::portal_permissions::ensure_portal_permissions();

    // Auto-configure shortcuts so they work out of the box on all desktops.
    // Best-effort: don't abort the install if hotkey setup fails.
    let app_config = apexshot::config::load_config();
    if let Err(e) = apexshot::hotkeys::sync_hotkeys_from_app_config(&app_config) {
        eprintln!("Warning: failed to write compositor hotkey snippets: {e}");
    }
    if let Err(e) = apexshot::hotkeys::sync_gnome_hotkeys_for_current_desktop(None) {
        eprintln!("Warning: GNOME hotkey setup skipped: {e}");
    }
}

pub(crate) fn run_uninstall(args: &[String]) {
    let mut autostart_only = false;
    let mut dev_install = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--autostart-only" => {
                autostart_only = true;
                i += 1;
            }
            "--dev" => {
                dev_install = true;
                i += 1;
            }
            other => {
                eprintln!("Error: unknown uninstall option '{other}'");
                std::process::exit(1);
            }
        }
    }

    uninstall_autostart(dev_install);

    if !autostart_only {
        if !dev_install && uninstall_package_managed_app_if_present() {
            return;
        }
        uninstall_binary(dev_install);
        uninstall_desktop_launcher(dev_install);
        if let Err(e) = super::native_host::uninstall_native_host_manifest(
            super::native_host::BrowserTarget::Both,
        ) {
            eprintln!("Error: failed to uninstall native host: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn command_exists(command: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn is_running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim() == "0")
        })
        .unwrap_or(false)
}

pub(crate) fn pacman_has_apexshot_package() -> bool {
    std::process::Command::new("pacman")
        .args(["-Qq", "apexshot"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn dpkg_has_apexshot_package() -> bool {
    std::process::Command::new("dpkg-query")
        .args(["-W", "-f=${Status}", "apexshot"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("install ok installed")
        })
        .unwrap_or(false)
}

pub(crate) fn rpm_has_apexshot_package() -> bool {
    std::process::Command::new("rpm")
        .args(["-q", "apexshot"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn os_release_field(field: &str) -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key != field {
            return None;
        }
        Some(value.trim_matches('"').to_string())
    })
}

pub(crate) fn rpm_package_manager() -> &'static str {
    let id = os_release_field("ID")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id_like = os_release_field("ID_LIKE")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let distro = format!(" {id} {id_like} ");

    if distro.contains(" opensuse ") || distro.contains(" suse ") || distro.contains(" sles ") {
        return "zypper";
    }
    if distro.contains(" fedora ")
        || distro.contains(" rhel ")
        || distro.contains(" centos ")
        || distro.contains(" rocky ")
        || distro.contains(" alma ")
    {
        return "dnf";
    }

    if command_exists("dnf") {
        "dnf"
    } else {
        "zypper"
    }
}

pub(crate) fn package_uninstall_command_for(
    manager: &str,
    needs_sudo: bool,
) -> Option<(String, Vec<String>)> {
    match (manager, needs_sudo) {
        ("pacman", true) => Some((
            "sudo".into(),
            vec!["pacman".into(), "-R".into(), "apexshot".into()],
        )),
        ("pacman", false) => Some(("pacman".into(), vec!["-R".into(), "apexshot".into()])),
        ("apt", true) => Some((
            "sudo".into(),
            vec!["apt".into(), "remove".into(), "apexshot".into()],
        )),
        ("apt", false) => Some(("apt".into(), vec!["remove".into(), "apexshot".into()])),
        ("dnf", true) => Some((
            "sudo".into(),
            vec![
                "dnf".into(),
                "remove".into(),
                "-y".into(),
                "apexshot".into(),
            ],
        )),
        ("dnf", false) => Some((
            "dnf".into(),
            vec!["remove".into(), "-y".into(), "apexshot".into()],
        )),
        ("zypper", true) => Some((
            "sudo".into(),
            vec![
                "zypper".into(),
                "--non-interactive".into(),
                "remove".into(),
                "apexshot".into(),
            ],
        )),
        ("zypper", false) => Some((
            "zypper".into(),
            vec![
                "--non-interactive".into(),
                "remove".into(),
                "apexshot".into(),
            ],
        )),
        _ => None,
    }
}

pub(crate) fn package_uninstall_command(manager: &str) -> Option<(String, Vec<String>)> {
    package_uninstall_command_for(manager, !is_running_as_root())
}

pub(crate) fn uninstall_package_managed_app_if_present() -> bool {
    let packaged_binary = std::path::Path::new("/usr/bin/apexshot");
    let packaged_capture = std::path::Path::new("/usr/bin/apexshot-capture");
    if !packaged_binary.exists() && !packaged_capture.exists() {
        return false;
    }

    let manager = if command_exists("pacman") && pacman_has_apexshot_package() {
        Some("pacman")
    } else if command_exists("dpkg-query") && dpkg_has_apexshot_package() {
        Some("apt")
    } else if command_exists("rpm") && rpm_has_apexshot_package() {
        Some(rpm_package_manager())
    } else {
        None
    };

    let Some(manager) = manager else {
        eprintln!("Error: package-managed ApexShot files exist under /usr/bin, but no supported package manager owns an installed 'apexshot' package.");
        eprintln!("Remove ApexShot with your distribution package manager, or remove the package files manually.");
        std::process::exit(1);
    };

    let Some((program, args)) = package_uninstall_command(manager) else {
        eprintln!("Error: unsupported package manager for ApexShot uninstall: {manager}");
        std::process::exit(1);
    };

    println!("Package-managed ApexShot install detected; uninstalling with {manager}.");
    let status = std::process::Command::new(&program).args(&args).status();
    match status {
        Ok(status) if status.success() => {
            println!("✓ Package-managed ApexShot removed");
            true
        }
        Ok(status) => {
            eprintln!("Error: package uninstall failed with status {status}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to run package uninstall command: {e}");
            std::process::exit(1);
        }
    }
}

/// Query an installed apexshot binary for its version string.
/// Returns `None` if the binary cannot be executed or the version cannot be parsed.
pub(crate) fn get_installed_version(binary: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Expected format: "apexshot 0.2.14"
    stdout
        .trim()
        .strip_prefix("apexshot ")
        .map(|v| v.to_string())
}

pub(crate) fn install_binary(force: bool, dev_install: bool) {
    use std::os::unix::fs::PermissionsExt;

    let dest = if dev_install {
        std::path::Path::new("/usr/local/lib/apexshot-dev/apexshot")
    } else {
        std::path::Path::new("/usr/local/bin/apexshot")
    };
    let capture_dest = if dev_install {
        std::path::Path::new("/usr/local/lib/apexshot-dev/apexshot-capture")
    } else {
        std::path::Path::new("/usr/local/bin/apexshot-capture")
    };
    let packaged_dest = std::path::Path::new("/usr/bin/apexshot");
    let packaged_capture_dest = std::path::Path::new("/usr/bin/apexshot-capture");
    let dev_wrapper = std::path::Path::new(app_identity::DEV_WRAPPER);

    let src = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("target/release/apexshot"));

    let current_version = env!("CARGO_PKG_VERSION");

    if !dev_install && (packaged_dest.exists() || packaged_capture_dest.exists()) {
        eprintln!("Error: package-managed ApexShot binaries exist under /usr/bin.");
        eprintln!(
            "`apexshot install` would write to /usr/local/bin/apexshot, which shadows the distro-managed installation."
        );
        eprintln!("Use `sudo apexshot install --dev --no-autostart` for a separate test install,");
        if command_exists("rpm") && rpm_has_apexshot_package() {
            let manager = rpm_package_manager();
            if manager == "dnf" {
                eprintln!("or update the package-managed app with `sudo dnf upgrade apexshot` or an updated RPM.");
            } else {
                eprintln!("or update the package-managed app with `sudo zypper update apexshot` or an updated RPM.");
            }
        } else {
            eprintln!("or update the package-managed app with your distro package manager.");
        }
        std::process::exit(1);
    }

    // Check if an existing installation is present and compare versions.
    if dest.exists() && !force {
        let installed_version = get_installed_version(dest);
        match installed_version {
            Some(ref v) if v == current_version => {
                if dev_install {
                    println!(
                        "Refreshing ApexShot Dev {} at {}.",
                        current_version,
                        dest.display()
                    );
                } else {
                    println!(
                        "ApexShot {} is already installed at {}. Use --force to reinstall.",
                        current_version,
                        dest.display()
                    );
                    return;
                }
            }
            Some(ref v) => {
                println!("Updating ApexShot {} → {}", v, current_version);
            }
            None => {
                // Could not determine version — proceed with install (likely a dev build or corrupted).
                println!(
                    "Existing installation found at {}. Updating to {}.",
                    dest.display(),
                    current_version
                );
            }
        }
    }

    println!("Installing binary: {} → {}", src.display(), dest.display());

    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "Error: failed to create install directory {}: {e}",
                parent.display()
            );
            std::process::exit(1);
        }
    }

    // Remove first to avoid ETXTBUSY when overwriting a running binary
    let _ = std::fs::remove_file(dest);

    match std::fs::copy(&src, dest) {
        Ok(_) => {
            if let Err(e) = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)) {
                eprintln!("Warning: could not set executable permissions: {e}");
            } else {
                println!("✓ Binary installed to {}", dest.display());
            }
        }
        Err(e) => {
            eprintln!("Error: failed to install binary: {e}");
            eprintln!("Hint: try running with sudo, e.g.  sudo apexshot install");
            std::process::exit(1);
        }
    }

    let capture_src = src
        .with_file_name("apexshot-capture")
        .exists()
        .then(|| src.with_file_name("apexshot-capture"))
        .or_else(|| {
            option_env!("APEXSHOT_CAPTURE_BIN_DIR").and_then(|dir| {
                let candidate = std::path::PathBuf::from(dir).join("apexshot-capture");
                candidate.exists().then_some(candidate)
            })
        });

    let Some(capture_src) = capture_src else {
        eprintln!("Error: apexshot-capture binary not found next to the built binary");
        eprintln!(
            "Hint: build from the backend directory so the C++ helper is compiled before install"
        );
        std::process::exit(1);
    };

    println!(
        "Installing capture helper: {} → {}",
        capture_src.display(),
        capture_dest.display()
    );

    if let Some(parent) = capture_dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "Error: failed to create install directory {}: {e}",
                parent.display()
            );
            std::process::exit(1);
        }
    }

    let _ = std::fs::remove_file(capture_dest);

    match std::fs::copy(&capture_src, capture_dest) {
        Ok(_) => {
            if let Err(e) =
                std::fs::set_permissions(capture_dest, std::fs::Permissions::from_mode(0o755))
            {
                eprintln!("Warning: could not set capture helper executable permissions: {e}");
            } else {
                println!("✓ Capture helper installed to {}", capture_dest.display());
            }
        }
        Err(e) => {
            eprintln!("Error: failed to install capture helper: {e}");
            eprintln!("Hint: try running with sudo, e.g.  sudo apexshot install");
            std::process::exit(1);
        }
    }

    if dev_install {
        let wrapper_content = format!(
            "#!/usr/bin/env bash\nexport APEXSHOT_APP_FLAVOR=dev\nexport APEXSHOT_CAPTURE_BIN=\"{}\"\nexec \"{}\" \"$@\"\n",
            capture_dest.display(),
            dest.display()
        );
        if let Err(e) = std::fs::write(dev_wrapper, wrapper_content) {
            eprintln!(
                "Error: failed to write dev wrapper {}: {e}",
                dev_wrapper.display()
            );
            std::process::exit(1);
        }
        if let Err(e) =
            std::fs::set_permissions(dev_wrapper, std::fs::Permissions::from_mode(0o755))
        {
            eprintln!("Warning: could not set dev wrapper executable permissions: {e}");
        } else {
            println!("✓ Dev wrapper installed to {}", dev_wrapper.display());
        }
    }
}

pub(crate) fn install_autostart(dev_install: bool) {
    // Clean up stale desktop files from previous `apexshot install` runs.
    // The .deb package installs the proper desktop entry to /usr/share/applications/,
    // but older versions of `apexshot install` wrote one to ~/.local/share/applications/
    // which takes priority and can point to a non-existent binary path.
    {
        let local_apps_dir = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".local/share")
            })
            .join("applications");

        let stale_desktop = local_apps_dir.join("io.github.codegoddy.apexshot.desktop");
        if stale_desktop.exists() {
            let _ = std::fs::remove_file(&stale_desktop);
            eprintln!(
                "[install] Removed stale desktop entry: {}",
                stale_desktop.display()
            );
        }
    }

    let autostart_dir = {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".config")
            });
        config_home.join("autostart")
    };

    if let Err(e) = std::fs::create_dir_all(&autostart_dir) {
        eprintln!("Error: could not create autostart directory: {e}");
        std::process::exit(1);
    }

    let binary_path = if dev_install {
        app_identity::DEV_WRAPPER.to_string()
    } else if std::path::Path::new("/usr/bin/apexshot").exists() {
        "/usr/bin/apexshot".to_string()
    } else if std::path::Path::new("/usr/local/bin/apexshot").exists() {
        "/usr/local/bin/apexshot".to_string()
    } else {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "apexshot".to_string())
    };

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment=ApexShot screenshot daemon - tray icon and hotkey listener\n\
         Exec={binary_path} daemon\n\
         Icon={}\n\
         Categories=Utility;\n\
         Keywords=screenshot;capture;record;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=2\n\
         Hidden=false\n\
         NoDisplay=true\n",
        if dev_install {
            "ApexShot Dev Daemon"
        } else {
            "ApexShot Daemon"
        },
        if dev_install {
            app_identity::DEV_APP_ID
        } else {
            app_identity::OFFICIAL_APP_ID
        },
    );

    let desktop_path = autostart_dir.join(if dev_install {
        "apexshot-dev-daemon.desktop"
    } else {
        "apexshot-daemon.desktop"
    });
    match std::fs::write(&desktop_path, &desktop_content) {
        Ok(()) => println!("✓ Autostart entry installed: {}", desktop_path.display()),
        Err(e) => {
            eprintln!("Error: failed to write autostart file: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn user_home_from_passwd(username: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("getent")
        .arg("passwd")
        .arg(username)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?;
    let home = line.split(':').nth(5)?;
    if home.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(home))
    }
}

pub(crate) fn install_desktop_launcher(dev_install: bool) {
    if !dev_install {
        return;
    }

    let local_apps_dir = if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
        // `apexshot install --dev` is normally run via sudo to copy files into
        // /usr/local.  Do not install the launcher into /root; put it in the
        // invoking user's desktop database so Hyprland/app launchers can see it.
        user_home_from_passwd(&sudo_user)
            .unwrap_or_else(|| std::path::PathBuf::from("/home").join(&sudo_user))
            .join(".local/share/applications")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".local/share")
            })
            .join("applications")
    };

    if let Err(e) = std::fs::create_dir_all(&local_apps_dir) {
        eprintln!("Error: could not create applications directory: {e}");
        std::process::exit(1);
    }

    let desktop_path = local_apps_dir.join("io.github.codegoddy.apexshot.dev.desktop");
    let desktop_content = format!(
        "[Desktop Entry]\n\
         Name=ApexShot Dev\n\
         Comment=Development build of ApexShot\n\
         Exec={}\n\
         Icon=apexshot\n\
         Type=Application\n\
         Categories=Graphics;\n\
         Keywords=screenshot;capture;recording;screen;video;ocr;annotation;\n\
         StartupNotify=true\n\
         StartupWMClass={}\n\
         Terminal=false\n\
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n\
         X-KDE-Wayland-Interfaces=zkde_screencast_unstable_v1\n",
        app_identity::DEV_WRAPPER,
        app_identity::DEV_APP_ID,
    );

    match std::fs::write(&desktop_path, desktop_content) {
        Ok(()) => {
            if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
                let _ = std::process::Command::new("chown")
                    .arg(format!(
                        "{}:{}",
                        sudo_user.to_string_lossy(),
                        sudo_user.to_string_lossy()
                    ))
                    .arg(&desktop_path)
                    .status();
            }
            println!("✓ Dev app launcher installed: {}", desktop_path.display())
        }
        Err(e) => {
            eprintln!("Error: failed to write dev launcher: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn uninstall_autostart(dev_install: bool) {
    let autostart_dir = {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .expect("HOME is not set");
                home.join(".config")
            });
        config_home.join("autostart")
    };
    let names: &[&str] = if dev_install {
        &["apexshot-dev-daemon.desktop"]
    } else {
        &["apexshot-daemon.desktop"]
    };
    for name in names {
        let desktop_path = autostart_dir.join(name);
        match std::fs::remove_file(&desktop_path) {
            Ok(()) => println!("✓ Autostart entry removed: {}", desktop_path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Autostart entry not found: {}", desktop_path.display());
            }
            Err(e) => {
                eprintln!("Error: failed to remove autostart file: {e}");
                std::process::exit(1);
            }
        }
    }
}

pub(crate) fn uninstall_binary_paths(dev_install: bool) -> &'static [&'static str] {
    if dev_install {
        &[
            app_identity::DEV_WRAPPER,
            "/usr/local/lib/apexshot-dev/apexshot",
            "/usr/local/lib/apexshot-dev/apexshot-capture",
        ]
    } else {
        &[
            "/usr/local/bin/apexshot",
            "/usr/local/bin/apexshot-capture",
            "/usr/local/bin/apexshot-native-host",
        ]
    }
}

pub(crate) fn uninstall_binary(dev_install: bool) {
    for path in uninstall_binary_paths(dev_install) {
        match std::fs::remove_file(path) {
            Ok(()) => println!("✓ Removed {}", path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("Error: failed to remove {path}: {e}");
                std::process::exit(1);
            }
        }
    }
}

pub(crate) fn uninstall_desktop_launcher(dev_install: bool) {
    if !dev_install {
        return;
    }

    let Some(mut desktop_path) = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
    else {
        return;
    };
    desktop_path.push("applications/io.github.codegoddy.apexshot.dev.desktop");

    match std::fs::remove_file(&desktop_path) {
        Ok(()) => println!("✓ Removed {}", desktop_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!("Error: failed to remove {}: {e}", desktop_path.display());
            std::process::exit(1);
        }
    }
}
