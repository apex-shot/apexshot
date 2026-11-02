//! Shared helpers for the Windows image editor.

use gtk4::gdk;
use gtk4::{prelude::*, Button, CssProvider, Image};
use std::path::Path;

/// Minimum window width at 100% scale. The editor keeps the Linux build's
/// floor so its toolbar and inspector still fit beside the canvas.
pub(super) const MIN_WINDOW_WIDTH: f64 = 980.0;
/// Minimum window height at 100% scale.
pub(super) const MIN_WINDOW_HEIGHT: f64 = 560.0;
/// Strip above the canvas reserved for the floating tool bar and title drag.
pub(super) const TOP_CHROME_HEIGHT: f64 = 56.0;
/// Width of the right inspector column at 100% scale, the same 288px dock the
/// Linux Motion inspector and the Windows video editor's tools column use.
pub(super) const INSPECTOR_WIDTH: f64 = 288.0;
const DEFAULT_WINDOW_WIDTH: f64 = 1560.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 900.0;

const IMAGE_EDITOR_CSS: &str = include_str!("image_editor.css");

pub(super) fn install_css() {
    crate::settings::install_window_css();
    // The Motion timeline reuses the shared recording editor stylesheet for
    // its dock, card and lane chrome.
    crate::video_editor::ui_support::install_recording_editor_css();

    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        IMAGE_EDITOR_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
    );
    crate::settings::shell::install_select_menu_css();
}

/// An empty overlay that hosts the editor's mode pages and overlays.
pub(super) fn editor_overlay() -> gtk4::Overlay {
    gtk4::Overlay::new()
}

/// Scale a logical size to device pixels.
pub(super) fn px(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

/// A flat icon button for the floating tool bar.
pub(super) fn tool_button(icon: &str, tooltip: &str, icon_size: i32) -> Button {
    let button = Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("image-editor-tool-button");
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(icon_size);
    button.set_child(Some(&image));
    button
}

/// A hairline between tool groups in the floating bar. The CSS margins keep
/// it a thin rule inside the bar's padding, like the Linux tool divider.
pub(super) fn toolbar_rule(_scale: f64) -> gtk4::Box {
    let rule = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    rule.add_css_class("image-editor-toolbar-rule");
    rule.set_vexpand(true);
    rule
}

/// Formats the editor opens: the same set the Linux editor accepts.
pub(super) fn is_supported_image(path: &Path) -> bool {
    path.is_file() && has_supported_image_extension(path)
}

fn has_supported_image_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp")
    )
}

/// The editor's default size, clamped to the monitor the window opens on.
pub(super) fn default_window_size(scale: f64) -> (i32, i32) {
    let requested = (
        px(DEFAULT_WINDOW_WIDTH, scale),
        px(DEFAULT_WINDOW_HEIGHT, scale),
    );
    let Some(display) = gdk::Display::default() else {
        return requested;
    };
    let Some(monitor) = display.monitors().item(0).and_downcast::<gdk::Monitor>() else {
        return requested;
    };
    let geometry = monitor.geometry();
    let max_width = (f64::from(geometry.width()) * 0.92).round() as i32;
    let max_height = (f64::from(geometry.height()) * 0.90).round() as i32;
    (
        requested.0.min(max_width).max(1),
        requested.1.min(max_height).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{has_supported_image_extension, is_supported_image};
    use std::path::Path;

    #[test]
    fn only_the_shared_image_formats_are_dropped() {
        assert!(has_supported_image_extension(Path::new("shot.png")));
        assert!(has_supported_image_extension(Path::new("SHOT.JPG")));
        assert!(has_supported_image_extension(Path::new("shot.jpeg")));
        assert!(has_supported_image_extension(Path::new("shot.webp")));
        assert!(!has_supported_image_extension(Path::new("shot.mp4")));
        assert!(!has_supported_image_extension(Path::new("shot")));
    }

    #[test]
    fn missing_files_are_rejected() {
        assert!(!is_supported_image(Path::new("does-not-exist.png")));
    }
}
