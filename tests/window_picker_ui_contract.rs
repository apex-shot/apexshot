#[test]
fn window_tool_removed_from_toolbars() {
    let cpp_drawing = include_str!("../capture-overlay/src/CaptureOverlay_Drawing.cpp");
    let cpp_events = include_str!("../capture-overlay/src/CaptureOverlay_Events.cpp");
    let rust_icons = include_str!("../src/overlay/icons.rs");
    let rust_window = include_str!("../src/overlay/window.rs");

    assert!(
        cpp_drawing.contains("\"Area\", \"Fullscreen\", \"Scroll\"")
            || cpp_drawing.contains("\"Area\", \"Fullscreen\", \"Scroll\", \"Timer\""),
        "C++ toolbar labels must not include Window"
    );
    assert!(
        !cpp_drawing.contains("\"Window\""),
        "C++ toolbar must not list Window as a tool label"
    );
    assert!(
        !cpp_events.contains("Window tool ignored") && !cpp_events.contains("enterWindowMode()"),
        "C++ toolbar click handler must not keep a Window tool branch"
    );

    assert!(
        rust_icons.contains("ToolbarIcon::Scroll") && !rust_icons.contains("ToolbarIcon::Window,"),
        "Rust TOOLBAR_ICONS must not include Window"
    );
    assert!(
        !rust_window.contains("ToolbarIcon::Window"),
        "Rust overlay click handler must not handle a Window toolbar tool"
    );
}
