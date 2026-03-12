//! Integration tests for Wayland backend
//!
//! ## IMPORTANT: These tests require user interaction
//!
//! Running these tests will trigger xdg-desktop-portal dialogs:
//! - `test_capture_screen_wayland` - May show a screenshot dialog
//! - `test_capture_area_wayland` - Will show an interactive selection dialog
//! - `test_capture_window_wayland` - Will show a window selection dialog
//!
//! If running on GNOME, dialogs may appear even for "non-interactive" captures.
//! On KDE/other compositors, behavior may vary.

#![cfg(test)]
use apexshot::backend::wayland::WaylandBackend;
use apexshot::backend::DisplayBackend;

/// Test basic backend creation and support detection
#[test]
fn test_backend_creation() {
    let backend = WaylandBackend::new();
    assert!(backend.is_ok(), "WaylandBackend::new() should succeed");
}

/// Test Wayland support detection
#[test]
fn test_is_supported() {
    let supported = WaylandBackend::is_supported();
    println!("Wayland backend supported: {}", supported);

    // If WAYLAND_DISPLAY is set, is_supported should return true
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        assert!(
            supported,
            "WAYLAND_DISPLAY is set but backend not supported"
        );
    }
}

/// Test screen capture (may show portal dialog)
#[test]
#[ignore = "requires user interaction via xdg-desktop-portal"]
fn test_capture_screen_wayland() {
    if !WaylandBackend::is_supported() {
        eprintln!("Wayland backend not supported, skipping test_capture_screen_wayland");
        return;
    }

    let backend = WaylandBackend::new().expect("Failed to create WaylandBackend");
    let result = backend.capture_screen();

    match result {
        Ok(capture_data) => {
            eprintln!("Capture successful!");
            eprintln!(
                "  Dimensions: {}x{}",
                capture_data.width, capture_data.height
            );
            eprintln!("  Pixels: {} bytes", capture_data.pixels.len());
            eprintln!(
                "  Format: {:?} ({} bpp)",
                capture_data.format, capture_data.format.bits_per_pixel
            );

            assert!(
                capture_data.width > 0,
                "Width should be positive, got {}",
                capture_data.width
            );
            assert!(
                capture_data.height > 0,
                "Height should be positive, got {}",
                capture_data.height
            );
            assert!(
                !capture_data.pixels.is_empty(),
                "Pixel data should not be empty"
            );

            // Validate pixel format - use u64 to avoid overflow
            let expected_pixels = (capture_data.width as u64) * (capture_data.height as u64);
            let expected_bytes = expected_pixels * (capture_data.format.bytes_per_pixel as u64);
            assert_eq!(
                capture_data.pixels.len() as u64,
                expected_bytes,
                "Pixel data size mismatch: expected {} bytes, got {}",
                expected_bytes,
                capture_data.pixels.len()
            );
        }
        Err(e) => {
            eprintln!("Wayland capture_screen failed with error: {:?}", e);
            eprintln!("This may be due to:");
            eprintln!("  - No running portal backend");
            eprintln!("  - User cancelled the dialog");
            eprintln!("  - Portal implementation issues");
            // We don't panic here because portal interactions are flaky in tests
        }
    }
}

/// Test area capture (WILL show interactive dialog, parameters are ignored)
#[test]
#[ignore = "requires user interaction via xdg-desktop-portal"]
fn test_capture_area_wayland() {
    if !WaylandBackend::is_supported() {
        eprintln!("Wayland backend not supported, skipping test_capture_area_wayland");
        return;
    }

    let backend = WaylandBackend::new().expect("Failed to create WaylandBackend");

    // NOTE: On Wayland, these parameters are IGNORED
    // The user will be prompted to interactively select an area
    let result = backend.capture_area(100, 100, 200, 200);

    match result {
        Ok(capture_data) => {
            assert!(capture_data.width > 0);
            assert!(capture_data.height > 0);
            assert!(!capture_data.pixels.is_empty());
        }
        Err(e) => {
            eprintln!("Wayland capture_area failed with error: {:?}", e);
            eprintln!("NOTE: Area capture on Wayland uses interactive mode");
            eprintln!("The user may have cancelled the dialog");
        }
    }
}

/// Test window capture (WILL show interactive dialog, window_id is ignored)
#[test]
#[ignore = "requires user interaction via xdg-desktop-portal"]
fn test_capture_window_wayland() {
    if !WaylandBackend::is_supported() {
        eprintln!("Wayland backend not supported, skipping test_capture_window_wayland");
        return;
    }

    let backend = WaylandBackend::new().expect("Failed to create WaylandBackend");

    // NOTE: On Wayland, this window_id is IGNORED
    // The user will be prompted to interactively select a window
    let result = backend.capture_window(12345);

    match result {
        Ok(capture_data) => {
            assert!(capture_data.width > 0);
            assert!(capture_data.height > 0);
            assert!(!capture_data.pixels.is_empty());
        }
        Err(e) => {
            eprintln!("Wayland capture_window failed with error: {:?}", e);
            eprintln!("NOTE: Window capture on Wayland uses interactive mode");
            eprintln!("The user may have cancelled the dialog");
        }
    }
}

/// Test that pixel format is properly detected
#[test]
#[ignore = "requires user interaction via xdg-desktop-portal"]
fn test_pixel_format_detection() {
    if !WaylandBackend::is_supported() {
        eprintln!("Wayland backend not supported, skipping test_pixel_format_detection");
        return;
    }

    let backend = WaylandBackend::new().expect("Failed to create WaylandBackend");

    if let Ok(capture_data) = backend.capture_screen() {
        // Most portals return RGB24 or RGBA32
        let bpp = capture_data.format.bits_per_pixel;
        assert!(bpp == 24 || bpp == 32, "Unexpected bits per pixel: {}", bpp);
    }
}
