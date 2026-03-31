/// Compile the C++ Qt5 capture overlay binary using CMake.
/// The compiled binary is placed in OUT_DIR and its location is exported
/// via the APEXSHOT_CAPTURE_BIN_DIR env var (embedded at compile time
/// via `option_env!` in capture_overlay.rs).
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
    println!("cargo:rerun-if-changed=capture-overlay/src/CaptureOverlay_HitTest.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/WindowPickerOverlay.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/WindowPickerOverlay.h");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScreenCapture.cpp");
    println!("cargo:rerun-if-changed=capture-overlay/src/ScreenCapture.h");
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

    // Export the directory so capture_overlay.rs can find the binary at runtime
    // (via option_env!("APEXSHOT_CAPTURE_BIN_DIR"))
    println!(
        "cargo:rustc-env=APEXSHOT_CAPTURE_BIN_DIR={}",
        build_dir.display()
    );

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
    build_capture_overlay();

    relm4_icons_build::bundle_icons(
        "icon_names.rs",
        Some("com.apexshot.editor"),
        None::<&str>,
        None::<&str>,
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
            "fog",
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
        ],
    );
}
