//! The shared recording-editor stylesheet, without the Linux-only helper
//! module around it.

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
);

pub(super) fn install_recording_editor_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(RECORDING_EDITOR_CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
    );
}
