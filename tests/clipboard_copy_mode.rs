//! Every screenshot copy path must honor Settings → Screenshots →
//! "Clipboard copy behavior" (Image Only / File Path Only / File & Image).

#[test]
fn daemon_capture_copy_honors_configured_mode() {
    let source = include_str!("../src/daemon/capture_handlers.rs");
    let start = source
        .find("pub fn copy_screenshot_to_clipboard")
        .expect("daemon copy entry point");
    let handler = &source[start..];
    let end = handler.find("\n}\n").unwrap_or(handler.len());
    let handler = &handler[..end];
    assert!(
        handler.contains("after_capture_copy_file_to_clipboard"),
        "daemon copy must stay gated on the After-capture copy checkbox"
    );
    assert!(
        handler.contains("adv_clipboard_mode") && handler.contains("copy_screenshot_with_mode"),
        "daemon copy must follow the configured clipboard mode"
    );
}

#[test]
fn manual_copy_actions_honor_configured_mode() {
    for (name, source) in [
        (
            "preview overlay",
            include_str!("../src/capture/preview_overlay.rs"),
        ),
        (
            "image editor",
            include_str!("../src/capture/editor/window/events/output.rs"),
        ),
        ("history", include_str!("../src/history/actions.rs")),
    ] {
        assert!(
            source.contains("adv_clipboard_mode") && source.contains("copy_screenshot_with_mode"),
            "{name} Copy must follow the configured clipboard mode"
        );
    }
}
