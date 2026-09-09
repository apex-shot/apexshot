/// Find the `apexshot-capture` binary.
///
/// Search order:
/// 1. `APEXSHOT_CAPTURE_BIN` env variable (manual override).
/// 2. Installed system paths (/usr/bin, /usr/local/bin).
/// 3. Same directory as the currently-running executable.
/// 4. Debug build output directory embedded by build.rs via `APEXSHOT_CAPTURE_BIN_DIR`.
/// 5. Common target profile directories relative to the exe (handles `cargo run` edge cases).
/// 6. PATH lookup.
fn find_capture_binary() -> Option<PathBuf> {
    // 1. Env override — highest priority for manual testing
    if let Some(p) = std::env::var_os("APEXSHOT_CAPTURE_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via env: {}",
                path.display()
            );
            return Some(path);
        }
    }

    // 2. Installed system paths — for .deb and manual installations
    if PathBuf::from("/usr/bin/apexshot-capture").exists() {
        eprintln!("[capture_overlay] Found apexshot-capture at /usr/bin/apexshot-capture");
        return Some(PathBuf::from("/usr/bin/apexshot-capture"));
    }
    if PathBuf::from("/usr/local/bin/apexshot-capture").exists() {
        eprintln!("[capture_overlay] Found apexshot-capture at /usr/local/bin/apexshot-capture");
        return Some(PathBuf::from("/usr/local/bin/apexshot-capture"));
    }

    // 3. Same directory as the running executable — useful for installed bundles.
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_file_name("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture next to exe: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 4. Debug build-time output directory embedded by build.rs.
    if let Some(dir) = option_env!("APEXSHOT_CAPTURE_BIN_DIR") {
        let candidate = PathBuf::from(dir).join("apexshot-capture");
        if candidate.exists() {
            eprintln!(
                "[capture_overlay] Found apexshot-capture via build dir: {}",
                candidate.display()
            );
            return Some(candidate);
        }
    }

    // 4. Walk up from exe dir to find target/release or target/debug
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            for profile in &["release", "debug"] {
                let candidate = d.join(profile).join("apexshot-capture");
                if candidate.exists() {
                    eprintln!(
                        "[capture_overlay] Found apexshot-capture in target/{}: {}",
                        profile,
                        candidate.display()
                    );
                    return Some(candidate);
                }
            }
            let candidate = d.join("apexshot-capture");
            if candidate.exists() && candidate != exe.with_file_name("apexshot-capture") {
                eprintln!(
                    "[capture_overlay] Found apexshot-capture in parent dir: {}",
                    candidate.display()
                );
                return Some(candidate);
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }

    // 5. PATH
    eprintln!("[capture_overlay] Searching PATH for apexshot-capture");
    which_in_path("apexshot-capture")
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        })
    })
}
