use gtk4::{gdk, CssProvider};

const RECORDING_EDITOR_CSS: &str = concat!(
    "\n",
    include_str!("ui_support_css/01.css"),
    include_str!("ui_support_css/02.css"),
    include_str!("ui_support_css/03.css"),
    include_str!("ui_support_css/04.css"),
    include_str!("ui_support_css/05.css"),
    include_str!("ui_support_css/06.css"),
    include_str!("ui_support_css/07.css"),
    include_str!("ui_support_css/08.css"),
);

pub fn install_recording_editor_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(RECORDING_EDITOR_CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RECORDING_EDITOR_CSS;

    #[test]
    fn light_theme_chrome_matches_timeline() {
        assert!(RECORDING_EDITOR_CSS.contains(
            ".editor-theme-light .recording-editor-window-controls {\n                background: #ffffff;"
        ));
        assert!(RECORDING_EDITOR_CSS.contains(
            ".editor-theme-light.recording-editor-shell {\n                background: #ffffff;\n                border: 1px solid alpha(#111827, 0.18);"
        ));
        assert!(RECORDING_EDITOR_CSS
            .contains(".recording-editor-root scale slider {\n                min-width: 12px;"));
        assert!(RECORDING_EDITOR_CSS
            .contains(".recording-editor-root scale.recording-editor-timeline-zoom slider"));
        assert!(RECORDING_EDITOR_CSS.contains("box-shadow: none;"));
    }
}
