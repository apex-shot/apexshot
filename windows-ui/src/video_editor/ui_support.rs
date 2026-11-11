//! The shared recording-editor stylesheet, without the Linux-only helper
//! module around it.

use crate::css_scale::scale_px_lengths;
use gtk4::{gdk, CssProvider};

const RECORDING_EDITOR_CSS: &str = concat!(
    "\n",
    include_str!("../../../src/recording/editor/ui_support_css/01.css"),
    include_str!("../../../src/recording/editor/ui_support_css/02.css"),
    include_str!("../../../src/recording/editor/ui_support_css/03.css"),
    include_str!("../../../src/recording/editor/ui_support_css/04.css"),
    include_str!("../../../src/recording/editor/ui_support_css/05.css"),
    include_str!("../../../src/recording/editor/ui_support_css/06.css"),
    include_str!("../../../src/recording/editor/ui_support_css/07.css"),
    include_str!("../../../src/recording/editor/ui_support_css/08.css"),
    include_str!("../../../src/recording/editor/ui_support_css/09.css"),
    r#"
    popover.gradient-type-menu button.recording-editor-dropdown-item {
        min-height: 32px;
        padding: 0 8px;
        font-size: 14px;
        font-weight: 500;
    }
    popover.gradient-type-menu button.recording-editor-dropdown-item:focus,
    popover.gradient-type-menu button.recording-editor-dropdown-item:focus-visible,
    popover.gradient-type-menu button.recording-editor-dropdown-item:focus-within {
        outline: none;
        box-shadow: none;
    }
    popover.gradient-type-menu button.recording-editor-dropdown-item.selected {
        background: alpha(white, 0.08);
    }
    popover.gradient-type-menu button.recording-editor-dropdown-item:hover {
        background: alpha(white, 0.12);
    }
    .editor-theme-light popover.gradient-type-menu button.recording-editor-dropdown-item.selected {
        background: alpha(#111827, 0.07);
    }
    .editor-theme-light popover.gradient-type-menu button.recording-editor-dropdown-item:hover {
        background: alpha(#111827, 0.10);
    }
    /* Inspector cards: the image editor's raised section, uppercase title
       and chevron, so both editors' side panels fold the same way. */
    .video-editor-section {
        padding: 10px;
        background: rgba(255, 255, 255, 0.025);
        border-radius: 8px;
    }
    .editor-theme-light .video-editor-section {
        background: rgba(17, 24, 39, 0.025);
    }
    .video-editor-section-title {
        color: rgba(255, 255, 255, 0.46);
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.6px;
        margin-top: 4px;
    }
    .editor-theme-light .video-editor-section-title {
        color: rgba(17, 24, 39, 0.56);
    }
    .video-editor-disclosure-header {
        min-height: 24px;
    }
    .video-editor-disclosure-header:hover .video-editor-section-title {
        color: rgba(255, 255, 255, 0.72);
    }
    .editor-theme-light .video-editor-disclosure-header:hover .video-editor-section-title {
        color: rgba(17, 24, 39, 0.72);
    }
    .video-editor-disclosure-chevron {
        min-width: 12px;
        color: rgba(255, 255, 255, 0.5);
    }
    .editor-theme-light .video-editor-disclosure-chevron {
        color: rgba(17, 24, 39, 0.5);
    }
    "#,
);

pub(crate) fn install_recording_editor_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&scale_px_lengths(
        RECORDING_EDITOR_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
    );
}
