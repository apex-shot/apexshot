//! The video editor's type scale.
//!
//! Settings, onboarding, the image editor and History each author their type
//! at 100% and let [`crate::platform::ui_scale`] carry it to the display. This
//! is the video editor's sheet: the same DM Sans stack, the same 14px base,
//! 15px headings, 13px body and controls, and 11px eyebrows and badges.
//!
//! Only `font-size` values are scaled here. The window, side-panel and export
//! sheets already scale their spacing and control metrics, so running the
//! whole sheet through the length scaler would grow the layout twice.

use gtk4::gdk;
use gtk4::CssProvider;

const VIDEO_EDITOR_TYPE_CSS: &str = r#"
        /* Base */
        .video-editor-root {
            font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
            font-size: 14px;
        }
        .video-editor-root entry,
        .video-editor-root entry text {
            font-size: 14px;
        }
        .video-editor-root checkbutton label {
            font-size: 13px;
        }

        /* Title bar and section headings */
        .video-editor-root .video-editor-document-title,
        .video-editor-root .recording-editor-title,
        .video-editor-root .video-editor-panel-title,
        .video-editor-root .recording-editor-zoom-title,
        .video-editor-root .recording-editor-dialog-title,
        .video-editor-root .recording-editor-crop-name {
            font-size: 15px;
        }
        .video-editor-root .recording-editor-custom-title,
        .video-editor-root .recording-editor-media-upload-title,
        .video-editor-root .recording-editor-cursor-unavailable-title {
            font-size: 14px;
        }

        /* Panel body, hints and row labels */
        .video-editor-root .video-editor-panel-detail,
        .video-editor-root .video-editor-panel-placeholder,
        .video-editor-root .recording-editor-label,
        .video-editor-root .recording-editor-stage-chip-label,
        .video-editor-root .recording-editor-timeline-clock,
        .video-editor-root .recording-editor-empty-preview-hint,
        .video-editor-root .recording-editor-empty-track-prompt,
        .video-editor-root .recording-editor-estimate,
        .video-editor-root .recording-editor-aspect-label,
        .video-editor-root .recording-editor-player-clock,
        .video-editor-root .recording-editor-dropdown-label,
        .video-editor-root .recording-editor-dialog-body,
        .video-editor-root .recording-editor-cursor-unavailable-detail,
        .video-editor-root .recording-editor-media-upload-hint,
        .video-editor-root .recording-editor-zoom-classic-label,
        .video-editor-root .recording-editor-media-title,
        .video-editor-root .recording-editor-motion-preset-label,
        .video-editor-root .recording-editor-cursor-tab label,
        .video-editor-root .recording-editor-zoom-easing-btn label,
        .video-editor-root .recording-editor-zoom-mode-btn label,
        .video-editor-root .recording-editor-zoom-chip label,
        .video-editor-root .recording-editor-bg-tab label,
        .video-editor-root .recording-editor-bg-none-row label,
        .video-editor-root .recording-editor-bg-image-row label,
        .video-editor-root .recording-editor-bg-custom-row label,
        .video-editor-root .recording-editor-bg-custom-edit label,
        .video-editor-root .recording-editor-gradient-steps-header label,
        .video-editor-root .recording-editor-custom-value,
        .video-editor-root .recording-editor-clip-menu-row,
        .video-editor-root .video-editor-value-number,
        .video-editor-root .recording-editor-webcam-card-label,
        .video-editor-root .recording-editor-webcam-scope-btn,
        .video-editor-root .recording-editor-webcam-scope-btn label,
        .video-editor-root .recording-editor-webcam-time,
        .video-editor-root .recording-editor-webcam-time entry,
        .video-editor-root .recording-editor-webcam-value {
            font-size: 13px;
        }

        /* Buttons and menu rows */
        .video-editor-root button.recording-editor-timeline-tool,
        .video-editor-root .recording-editor-primary-button,
        .video-editor-root .recording-editor-primary-button label,
        .video-editor-root .recording-editor-secondary-button,
        .video-editor-root .recording-editor-ghost-button,
        .video-editor-root .recording-editor-ghost-button label,
        .video-editor-root .recording-editor-drop-label,
        .video-editor-root .recording-editor-dialog-root .recording-editor-primary-button,
        .video-editor-root .recording-editor-dialog-root .recording-editor-primary-button label,
        .video-editor-root .recording-editor-title-export label,
        .video-editor-root .recording-editor-zoom-reset label,
        .video-editor-root .video-editor-secondary-button {
            font-size: 13px;
        }

        /* Eyebrows, badges and dense readouts */
        .video-editor-root .recording-editor-panel-title,
        .video-editor-root .recording-editor-zoom-kicker,
        .video-editor-root .recording-editor-motion-preset-hint,
        .video-editor-root .recording-editor-media-empty,
        .video-editor-root .recording-editor-media-meta,
        .video-editor-root .recording-editor-time-label,
        .video-editor-root .recording-editor-convert-hint,
        .video-editor-root .recording-editor-dropdown-arrow,
        .video-editor-root .recording-editor-media-tab label,
        .video-editor-root .recording-editor-cursor-card-label,
        .video-editor-root .recording-editor-click-effect-label,
        .video-editor-root .recording-editor-click-color-hex,
        .video-editor-root .recording-editor-custom-unit,
        .video-editor-root .recording-editor-webcam-kicker,
        .video-editor-root .recording-editor-webcam-status,
        .video-editor-root .recording-editor-dim-badge {
            font-size: 11px;
        }

        /* Export dialog */
        .video-editor-root .video-export-options,
        .video-editor-root .video-export-screen {
            font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
            font-size: 14px;
        }
        .video-editor-root .video-export-dialog-title {
            font-size: 18px;
        }
        .video-editor-root .video-export-muted {
            font-size: 13px;
        }
        .video-editor-root .video-export-options button.video-export-action,
        .video-editor-root .video-export-options button.video-export-preset,
        .video-editor-root .video-export-options button.video-export-close,
        .video-editor-root .video-export-screen button,
        .video-editor-root .video-export-screen button.video-export-stop,
        .video-editor-root .video-export-options button.settings-select,
        .video-editor-root .video-export-options button.settings-select label {
            font-size: 14px;
        }
        .video-editor-root .video-export-destination {
            font-size: 13px;
        }
        .video-editor-root .video-export-progress-title {
            font-size: 15px;
        }

        /* Popovers and menus */
        popover.recording-editor-tool-rail-popover,
        popover.recording-editor-custom-popover,
        popover.settings-select-popover,
        popover.gradient-type-menu {
            font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
            font-size: 14px;
        }
        "#;

/// Install the video editor type sheet over the shared editor and window sheets.
pub(super) fn install() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_font_sizes(
        VIDEO_EDITOR_TYPE_CSS,
        crate::platform::ui_scale(),
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 6,
    );
}
