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

/// The editor's type scale, authored at 100% and scaled by
/// [`crate::platform::ui_scale`] when it is installed.
///
/// Settings and onboarding both author their type at 100% and let the display
/// scale carry it: Settings in a type sheet layered over its shell, onboarding
/// in its page sheet. This is the editor's version of that. Every role is
/// restated here, and [`crate::css_scale::scale_font_sizes`] multiplies only the
/// `font-size` values, so the type resolves from one place at the top of the
/// cascade. Weights, colour and every length stay with `image_editor.css`, whose
/// metrics [`install_css`] scales by the display.
///
/// The sizes are the Settings window's — a 14px base, 15px row titles, 13px
/// hints and buttons, and 11px eyebrow labels — so the two windows read alike.
const IMAGE_EDITOR_TYPE_CSS: &str = r#"
        /* Base */
        .image-editor-root {
            font-size: 14px;
        }

        /* Panel and section headings */
        .image-editor-inspector-title {
            font-size: 15px;
        }
        .image-editor-section-title {
            font-size: 11px;
        }
        .image-editor-frame-subheader {
            font-size: 14px;
        }
        .image-editor-custom-color-title {
            font-size: 13px;
        }

        /* Rows, tabs and disclosure actions */
        button.image-editor-bg-none-row,
        button.image-editor-bg-none-row label,
        button.image-editor-bg-custom-edit,
        button.image-editor-bg-custom-edit label,
        button.image-editor-bg-image-row,
        button.image-editor-bg-image-row label,
        .image-editor-bg-custom-row label {
            font-size: 14px;
        }
        togglebutton.image-editor-bg-tab,
        togglebutton.image-editor-bg-tab label,
        button.image-editor-preset-pill label {
            font-size: 13px;
        }
        button.image-editor-inline-button,
        button.image-editor-inline-button label,
        button.image-editor-watermark-remove,
        button.image-editor-watermark-remove label,
        button.image-editor-transform-reset,
        button.image-editor-transform-reset label {
            font-size: 13px;
        }
        .image-editor-frame-tile-label,
        .image-editor-frame-sublabel {
            font-size: 11px;
        }
        .image-editor-dim-unit {
            font-size: 12px;
        }
        entry.image-editor-dim-entry {
            font-size: 13px;
        }
        .image-editor-font-dropdown {
            font-size: 13px;
        }
        .image-editor-text-content,
        .image-editor-text-content text {
            font-size: 13px;
        }

        /* Floating bar and popovers */
        button.image-editor-subbar-pill,
        button.image-editor-subbar-button,
        entry.image-editor-subbar-entry {
            font-size: 13px;
        }
        button.image-editor-subbar-option {
            font-size: 15px;
        }
        .image-editor-subbar-dimensions {
            font-size: 12px;
        }
        .image-editor-number-style-sample {
            font-size: 14px;
        }
        .image-editor-effect-value {
            font-size: 13px;
        }
        .image-editor-custom-opacity-label,
        .image-editor-custom-opacity-value {
            font-size: 12px;
        }
        button.image-editor-motion-button,
        button.image-editor-mode-button,
        button.image-editor-mode-button label,
        button.image-editor-zoom-label {
            font-size: 13px;
        }
        button.image-editor-zoom-step {
            font-size: 16px;
        }
        button.image-editor-primary-button,
        button.image-editor-export,
        button.image-editor-export label {
            font-size: 13px;
        }

        /* Hints, overlays and empty states */
        .image-editor-hint,
        .image-editor-empty-detail,
        .image-editor-empty-error,
        .image-editor-empty-loading {
            font-size: 13px;
        }
        .image-editor-empty-title {
            font-size: 18px;
        }
        .image-editor-toast {
            font-size: 13px;
        }

        /* Mode-switch confirmation */
        .image-editor-confirm-title {
            font-size: 15px;
        }
        .image-editor-confirm-body,
        .image-editor-confirm-skip,
        button.image-editor-confirm-cancel,
        button.image-editor-confirm-continue {
            font-size: 13px;
        }
        "#;

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
    install_type_css();
    crate::settings::shell::install_select_menu_css();
}

/// Install the editor's type scale over the metrics sheet.
///
/// The sheet is authored at 100%, so only its `font-size` values are taken
/// through the display scaler; running the whole sheet through the length
/// scaler would grow its spacing a second time. Priority sits above the metrics
/// sheet (and above the shared Settings type sheet, which never matches an
/// `.image-editor-` selector) so these roles win the cascade.
fn install_type_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_font_sizes(
        IMAGE_EDITOR_TYPE_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
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
    use super::{has_supported_image_extension, is_supported_image, IMAGE_EDITOR_TYPE_CSS};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

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

    #[test]
    fn type_sheet_parses() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(RefCell::new(Vec::new()));
            let captured = Rc::clone(&errors);
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(IMAGE_EDITOR_TYPE_CSS);
            let result = errors.borrow().clone();
            result
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
        assert!(
            errors.is_empty(),
            "image editor type CSS errors: {errors:?}"
        );
    }

    #[test]
    fn type_sheet_scales_with_the_display() {
        let scaled = crate::css_scale::scale_font_sizes(IMAGE_EDITOR_TYPE_CSS, 1.5);
        // The 14px base and the 15px row titles follow the display.
        assert!(scaled.contains("font-size: 21px;"));
        assert!(scaled.contains("font-size: 22.5px;"));
        // Every role survives the pass; only the values change.
        assert_eq!(
            scaled.matches("font-size:").count(),
            IMAGE_EDITOR_TYPE_CSS.matches("font-size:").count()
        );
    }
}
