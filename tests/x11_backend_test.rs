use apexshot::backend::{x11::X11Backend, DisplayBackend};

// Skip tests if X11 is not available
fn should_skip_test() -> bool {
    if !X11Backend::is_supported() {
        println!("Skipping test - X11 not available");
        return true;
    }
    false
}

#[test]
fn test_x11_backend_creation() {
    if should_skip_test() {
        return;
    }

    let backend = X11Backend::new();
    assert!(backend.is_ok());
}

#[test]
fn test_screen_capture() {
    if should_skip_test() {
        return;
    }

    let backend = X11Backend::new().unwrap();
    let capture = match backend.capture_screen() {
        Ok(c) => c,
        Err(e) => {
            println!(
                "Failed to capture screen: {} (this is expected if no X server is running)",
                e
            );
            return;
        }
    };

    // Basic sanity checks
    assert!(capture.width > 0);
    assert!(capture.height > 0);
    assert_eq!(capture.pixels.len() as u32, capture.size_bytes());

    // Check pixel format
    let format = capture.format;
    assert!(matches!(format.bits_per_pixel, 24 | 32));
    assert!(matches!(format.bytes_per_pixel, 3 | 4));
    assert!(format.red_mask != 0);
    assert!(format.green_mask != 0);
    assert!(format.blue_mask != 0);
}

#[test]
fn test_area_capture() {
    if should_skip_test() {
        return;
    }

    let backend = X11Backend::new().unwrap();

    // Try to capture a 100x100 area at (0,0)
    let capture = match backend.capture_area(0, 0, 100, 100) {
        Ok(c) => c,
        Err(e) => {
            println!(
                "Failed to capture area: {} (this is expected if no X server is running)",
                e
            );
            return;
        }
    };

    assert_eq!(capture.width, 100);
    assert_eq!(capture.height, 100);
    assert_eq!(capture.pixels.len() as u32, capture.size_bytes());
}

#[test]
fn test_cursor_capture() {
    if should_skip_test() {
        return;
    }

    let backend = X11Backend::new().unwrap();
    let capture = match backend.capture_screen() {
        Ok(c) => c,
        Err(e) => {
            println!(
                "Failed to capture screen: {} (this is expected if no X server is running)",
                e
            );
            return;
        }
    };

    if let Some(cursor) = capture.cursor {
        // Basic cursor data validation
        assert!(cursor.width > 0);
        assert!(cursor.height > 0);
        assert_eq!(
            cursor.pixels.len(),
            (cursor.width * cursor.height * 4) as usize
        );

        // Cursor should be within screen bounds
        assert!(cursor.x >= -(cursor.width as i32));
        assert!(cursor.y >= -(cursor.height as i32));
        assert!(cursor.x < capture.width as i32);
        assert!(cursor.y < capture.height as i32);

        // Hotspot should be within cursor bounds
        assert!(cursor.xhot < cursor.width);
        assert!(cursor.yhot < cursor.height);
    } else {
        println!("No cursor captured (XFixes may not be available)");
    }
}

#[test]
fn test_invalid_captures() {
    if should_skip_test() {
        return;
    }

    let backend = X11Backend::new().unwrap();

    // Test invalid dimensions
    assert!(backend.capture_area(0, 0, 0, 100).is_err());
    assert!(backend.capture_area(0, 0, 100, 0).is_err());
    assert!(backend.capture_area(0, 0, -1, 100).is_err());
    assert!(backend.capture_area(0, 0, 100, -1).is_err());

    // Test out of bounds capture
    let screen = match backend.capture_screen() {
        Ok(s) => s,
        Err(e) => {
            println!(
                "Failed to capture screen: {} (this is expected if no X server is running)",
                e
            );
            return;
        }
    };
    let result = backend.capture_area(screen.width as i32, screen.height as i32, 100, 100);
    assert!(result.is_err());
}
