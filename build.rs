use std::collections::BTreeMap;
use std::path::Path;

/// Compile the C++ Qt5 capture overlay binary using CMake.
/// The compiled binary is placed in OUT_DIR. Debug builds also embed the
/// build directory for local development; release builds avoid embedding
/// absolute package build paths.
fn build_capture_overlay() {
    use std::path::PathBuf;
    use std::process::Command;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");

    let src_dir = PathBuf::from(&manifest_dir).join("capture-overlay");
    let build_dir = PathBuf::from(&out_dir).join("capture-overlay-build");

    // Tell Cargo to re-run this script if C++ sources change
    println!("cargo:rerun-if-changed=capture-overlay/CMakeLists.txt");
    println!("cargo:rerun-if-changed=capture-overlay/src/main.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_Drawing.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_Events.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_Scroll.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_HitTest.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_Window.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_p.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/RecordingControlsWindow.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/RecordingControlsWindow.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScrollControlPanel.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScrollControlPanel.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/WindowPickerOverlay.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/WindowPickerOverlay.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScreenCapture.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScreenCapture.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/MonitorPicker.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/MonitorPicker.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/request.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/request.h");

    // Create build dir
    std::fs::create_dir_all(&build_dir).expect("Failed to create C++ build dir");

    // cmake configure
    let cmake_status = Command::new("cmake")
        .arg(src_dir.to_str().expect("src path not UTF-8"))
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .current_dir(&build_dir)
        .status()
        .expect("cmake not found — install cmake");

    if !cmake_status.success() {
        panic!("cmake configure failed for capture-overlay");
    }

    // cmake build
    let nproc = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "4".into());

    let build_status = Command::new("cmake")
        .args(["--build", ".", "--", "-j"])
        .arg(&nproc)
        .current_dir(&build_dir)
        .status()
        .expect("cmake --build failed");

    if !build_status.success() {
        panic!("cmake build failed for capture-overlay");
    }

    // Export the directory only for debug/dev builds. Release package builds
    // should not contain absolute references to $srcdir.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        println!(
            "cargo:rustc-env=APEXSHOT_CAPTURE_BIN_DIR={}",
            build_dir.display()
        );
    }

    // Also copy the binary next to the Rust binary in the target directory
    // so it's available when running `cargo run` during development.
    let binary_src = build_dir.join("apexshot-capture");
    if binary_src.exists() {
        // Walk up from OUT_DIR to find target/{debug,release}/
        // OUT_DIR is typically: target/{profile}/build/<crate>-<hash>/out
        if let Some(target_dir) = PathBuf::from(&out_dir).ancestors().find(|p| {
            p.join("apexshot").exists()
                || p.file_name()
                    .map(|n| n == "debug" || n == "release")
                    .unwrap_or(false)
        }) {
            let dest = target_dir.join("apexshot-capture");
            let _ = std::fs::copy(&binary_src, &dest);
        }
    }
}

fn main() {
    compile_translations();

    // Flatpak builds are portal-only: skip the Qt5/X11 C++ helper entirely.
    if std::env::var_os("CARGO_FEATURE_FLATPAK").is_none() {
        build_capture_overlay();
    } else {
        println!("cargo:warning=flatpak feature: skipping Qt capture-overlay build");
    }

    // Rebuild whenever a custom SVG is added/modified in data/icons.
    println!("cargo:rerun-if-changed=data/icons");

    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        Some("data/icons"),
        [
            "crop",
            "go-next",
            "arrow-up-right-regular",
            "draw-line",
            "rectangle-landscape-regular",
            "circle-regular",
            "highlight-regular",
            "text-t-regular",
            "text-italic-regular",
            "view-grid",
            "blur",
            "shield-regular",
            "select",
            "chevron-down-regular",
            "chevron-right-regular",
            "small-rectangle-in-focus",
            "arrow-undo-regular",
            "arrow-redo-regular",
            "delete-regular",
            "pen-regular",
            "view-pin",
            "pin",
            "copy-regular",
            "cloud-arrow-up-regular",
            "number-circle-1-regular",
            "pointer-primary-click",
            "image-regular",
            "media-playback-stop",
            "dismiss-regular",
            "eyedropper-regular",
            "save-regular",
            "arrow-export-up-regular",
            "folder-open-regular",
        ],
    );
}

fn compile_translations() {
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    println!("cargo:rerun-if-changed=po");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let po_dir = manifest_dir.join("po");
    if !po_dir.is_dir() {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut locale_roots = vec![
        out_dir.join("locale"),
        manifest_dir.join("target").join("locale"),
    ];
    if let Some(target_dir) = PathBuf::from(&out_dir).ancestors().find(|p| {
        p.file_name()
            .map(|n| n == "debug" || n == "release")
            .unwrap_or(false)
    }) {
        locale_roots.push(target_dir.join("locale"));
    }

    let Ok(entries) = fs::read_dir(&po_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("po") {
            continue;
        }
        let Some(lang) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if lang == "apexshot" {
            continue;
        }
        let messages = match parse_po_file(&path) {
            Ok(messages) => messages,
            Err(err) => {
                println!("cargo:warning=failed to parse {}: {err}", path.display());
                continue;
            }
        };
        let mo = encode_mo(&messages);
        for root in &locale_roots {
            let dest_dir = root.join(lang).join("LC_MESSAGES");
            if fs::create_dir_all(&dest_dir).is_err() {
                continue;
            }
            let dest = dest_dir.join("apexshot.mo");
            if let Ok(mut file) = fs::File::create(&dest) {
                let _ = file.write_all(&mo);
            }
        }
    }
}

fn parse_po_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut messages = BTreeMap::new();
    let mut msgid = String::new();
    let mut msgstr = String::new();
    let mut state = PoState::None;

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() {
            finish_po_entry(&mut messages, &mut msgid, &mut msgstr, &mut state);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid ") {
            finish_po_entry(&mut messages, &mut msgid, &mut msgstr, &mut state);
            msgid = parse_po_quoted(rest)?;
            state = PoState::Msgid;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            msgstr = parse_po_quoted(rest)?;
            state = PoState::Msgstr;
        } else if line.starts_with('"') {
            let chunk = parse_po_quoted(line)?;
            match state {
                PoState::Msgid => msgid.push_str(&chunk),
                PoState::Msgstr => msgstr.push_str(&chunk),
                PoState::None => {}
            }
        }
    }
    finish_po_entry(&mut messages, &mut msgid, &mut msgstr, &mut state);
    Ok(messages)
}

#[derive(Clone, Copy)]
enum PoState {
    None,
    Msgid,
    Msgstr,
}

fn finish_po_entry(
    messages: &mut BTreeMap<String, String>,
    msgid: &mut String,
    msgstr: &mut String,
    state: &mut PoState,
) {
    if !msgid.is_empty() && !msgstr.is_empty() {
        messages.insert(std::mem::take(msgid), std::mem::take(msgstr));
    } else {
        msgid.clear();
        msgstr.clear();
    }
    *state = PoState::None;
}

fn parse_po_quoted(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let inner = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("expected quoted string, got {trimmed}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Ok(out)
}

fn encode_mo(messages: &BTreeMap<String, String>) -> Vec<u8> {
    let n = messages.len() as u32;
    let orig_tab = 28u32;
    let trans_tab = orig_tab + 8 * n;
    let strings_offset = trans_tab + 8 * n;

    let mut originals = Vec::new();
    let mut translations = Vec::new();
    for (id, value) in messages {
        originals.push(id.as_bytes());
        translations.push(value.as_bytes());
    }

    let mut original_data = Vec::new();
    let mut translation_data = Vec::new();
    let mut orig_meta = Vec::new();
    let mut trans_meta = Vec::new();
    let mut cursor = strings_offset;
    for bytes in &originals {
        orig_meta.push((bytes.len() as u32, cursor));
        original_data.extend_from_slice(bytes);
        original_data.push(0);
        cursor += bytes.len() as u32 + 1;
    }
    for bytes in &translations {
        trans_meta.push((bytes.len() as u32, cursor));
        translation_data.extend_from_slice(bytes);
        translation_data.push(0);
        cursor += bytes.len() as u32 + 1;
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(&0x9504_12deu32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&orig_tab.to_le_bytes());
    out.extend_from_slice(&trans_tab.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&strings_offset.to_le_bytes());
    for (len, offset) in orig_meta {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
    }
    for (len, offset) in trans_meta {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&original_data);
    out.extend_from_slice(&translation_data);
    out
}
