use gtk4::{gdk, CssProvider};

pub fn install_recording_editor_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            .recording-editor-root {
                padding: 0;
                color: #F1F1F3;
                font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
                min-width: 900px;
                min-height: 640px;
                background: #111111;
            }

            /* ── Strip native Adwaita/Ubuntu chrome from all descendants ── */
            .recording-editor-root button {
                background-image: none;
                text-shadow: none;
                box-shadow: none;
                -gtk-icon-shadow: none;
                border: none;
                outline: none;
            }

            .recording-editor-root entry {
                background-image: none;
                box-shadow: none;
                border: none;
                border-radius: 6px;
                background-color: alpha(white, 0.06);
                color: #F1F1F3;
                padding: 0 8px;
                min-height: 28px;
                outline: none;
            }

            .recording-editor-root entry text {
                color: #F1F1F3;
                font-size: 12px;
                background: transparent;
                caret-color: #F1F1F3;
            }

            .recording-editor-root entry:focus {
                background-color: alpha(white, 0.08);
            }

            .recording-editor-root entry:disabled {
                opacity: 0.52;
                background: alpha(white, 0.03);
                color: alpha(white, 0.42);
            }

            .recording-editor-root entry:disabled text {
                color: alpha(white, 0.42);
            }

            .recording-editor-root scale {
                color: #F1F1F3;
                min-height: 20px;
            }

            .recording-editor-root scale trough {
                min-height: 4px;
                border-radius: 999px;
                background: alpha(white, 0.08);
                border: none;
            }

            .recording-editor-root scale highlight {
                min-height: 4px;
                border-radius: 999px;
                background: #b05c38;
            }

            .recording-editor-root checkbutton {
                padding: 2px 0;
                color: alpha(white, 0.78);
                background: transparent;
                border: none;
                box-shadow: none;
                text-shadow: none;
                -gtk-icon-shadow: none;
            }

            .recording-editor-root checkbutton check {
                min-width: 16px;
                min-height: 16px;
                background-image: none;
                background-color: alpha(white, 0.06);
                border: 1px solid alpha(white, 0.16);
                color: #ffffff;
                box-shadow: none;
            }

            .recording-editor-root checkbutton check:hover {
                border-color: alpha(white, 0.28);
                background-color: alpha(white, 0.10);
            }

            .recording-editor-root checkbutton:checked check,
            .recording-editor-root checkbutton check:checked,
            .recording-editor-root checkbutton.recording-editor-audio-choice check:checked {
                background-color: #b05c38;
                border-color: #b05c38;
                color: #ffffff;
            }

            .recording-editor-root checkbutton:disabled check {
                background-color: alpha(white, 0.03);
                border-color: alpha(white, 0.10);
                color: alpha(white, 0.35);
            }

            .recording-editor-root checkbutton label {
                color: alpha(white, 0.78);
                font-size: 12px;
            }

            .recording-editor-root label {
                color: alpha(white, 0.85);
            }

            .recording-editor-root spinner {
                color: #F1F1F3;
            }

            /* ── Scrollbar ── */
            .recording-editor-root scrollbar slider {
                background-color: alpha(white, 0.18);
                border-radius: 999px;
                min-width: 12px;
                min-height: 12px;
                border: none;
            }

            .recording-editor-root scrollbar slider:hover {
                background-color: alpha(white, 0.28);
            }

            .recording-editor-root scrollbar slider:active {
                background-color: alpha(white, 0.35);
            }

            .recording-editor-root scrollbar trough {
                background: transparent;
                border: none;
            }

            /* ── Title bar ── */
            .recording-editor-window-controls {
                min-height: 0;
                padding: 0;
                background: transparent;
            }

            .recording-editor-window-controls.editor-toolbar {
                min-height: 0;
            }

            .recording-editor-title {
                color: alpha(white, 0.72);
                font-size: 12px;
                font-weight: 500;
            }

            .recording-editor-window-controls .editor-toolbar-left {
                margin-left: 8px;
                min-height: 0;
            }

            .recording-editor-root .editor-toolbar-left {
                min-height: 0;
            }

            .recording-editor-window-controls .editor-traffic-lights {
                margin-right: 6px;
                min-height: 0;
            }

            .recording-editor-traffic-btn {
                min-width: 24px;
                min-height: 24px;
                padding: 0;
                margin: 0;
                border-radius: 999px;
                background: transparent;
                background-image: none;
                color: alpha(white, 0.65);
                border: none;
                box-shadow: none;
                outline: none;
            }

            .recording-editor-traffic-btn image {
                -gtk-icon-size: 14px;
            }

            .recording-editor-traffic-btn:hover,
            .recording-editor-traffic-btn:active {
                background: alpha(white, 0.10);
                background-image: none;
                color: #ffffff;
                border-radius: 999px;
                border: none;
                box-shadow: none;
            }

            .recording-editor-traffic-btn:hover image,
            .recording-editor-traffic-btn:active image {
                color: #ffffff;
            }

            /* ── Preview ── */
            .recording-editor-preview-frame {
                background: #111111;
                min-height: 260px;
                padding: 0;
                margin: 0;
            }

            .recording-editor-preview-workspace {
                padding: 0;
                margin: 0;
                background: #111111;
            }

            .recording-editor-video {
                background: #111111;
                border-radius: 0;
                border: none;
                box-shadow: none;
                margin: 0;
                padding: 0;
            }

            .recording-editor-empty-workspace {
                min-height: 260px;
            }

            .recording-editor-empty-dropzone {
                min-width: 360px;
                padding: 0;
                background: transparent;
            }

            .recording-editor-empty-icon {
                color: alpha(white, 0.38);
            }

            .recording-editor-empty-title {
                color: #F1F1F3;
                font-size: 16px;
                font-weight: 700;
            }

            .recording-editor-empty-hint {
                color: alpha(white, 0.52);
                font-size: 12px;
            }

            .recording-editor-empty-open-button {
                min-width: 132px;
                margin-top: 6px;
            }

            .recording-editor-empty-thumbnail-strip {
                background: alpha(white, 0.035);
                border: 1px dashed alpha(white, 0.10);
            }

            .recording-editor-dim-badge {
                background: #b05c38;
                color: #ffffff;
                font-size: 11px;
                font-weight: 600;
                padding: 4px 10px;
                border-radius: 4px;
                border: 1px solid alpha(#111827, 0.08);
            }

            .recording-editor-dim-badge label,
            .recording-editor-dim-badge {
                color: #ffffff;
            }

            /* ── Bottom tools ── */
            .recording-editor-bottom-tools {
                padding: 0;
                margin: 0;
                background-color: #141414;
                border-top: 1px solid alpha(white, 0.06);
                border-radius: 0 0 10px 10px;
            }

            /* ── Timeline ── */
            .recording-editor-timeline {
                min-height: 48px;
                padding: 6px 14px;
                background: transparent;
            }

            .recording-editor-timeline-card {
                border-radius: 8px;
                background: alpha(white, 0.04);
                border: none;
                padding: 4px 8px;
            }

            .recording-editor-play-button,
            .recording-editor-cut-button,
            .recording-editor-revert-button {
                min-width: 32px;
                min-height: 32px;
                border-radius: 999px;
                background: alpha(white, 0.08);
                color: white;
                border: none;
            }

            .recording-editor-play-button {
                margin-right: 8px;
            }

            .recording-editor-timeline-tools {
                margin-left: 8px;
            }

            .recording-editor-play-button:hover,
            .recording-editor-cut-button:hover,
            .recording-editor-revert-button:hover,
            .recording-editor-cut-button-active {
                background: alpha(white, 0.14);
            }

            .recording-editor-cut-button-active {
                color: #f0a07a;
            }

            .recording-editor-play-button image,
            .recording-editor-cut-button image,
            .recording-editor-revert-button image {
                color: white;
            }

            .recording-editor-cut-button-active image {
                color: #f0a07a;
            }

            .recording-editor-thumbnail-strip {
                background: alpha(white, 0.06);
                border-radius: 4px;
                border: none;
                min-height: 36px;
                padding: 2px;
            }

            .recording-editor-thumbnail {
                min-width: 48px;
                min-height: 32px;
                background: alpha(white, 0.60);
                border-right: 1px solid alpha(black, 0.12);
            }

            .recording-editor-trim-area {
                min-height: 36px;
            }

            .recording-editor-trim-range {
                background: alpha(#b05c38, 0.15);
                border-top: 2px solid #b05c38;
                border-bottom: 2px solid #b05c38;
            }

            .recording-editor-trim-handle {
                min-width: 8px;
                background: #b05c38;
                border-radius: 3px;
                border: none;
            }

            .recording-editor-time-label {
                color: alpha(white, 0.45);
                font-size: 10px;
            }

            /* ── Panels ── */
            .recording-editor-panels {
                padding: 10px 14px 6px 14px;
                background: transparent;
                border: none;
            }

            .recording-editor-panel {
                padding: 0;
                border-radius: 0;
                background: transparent;
                border: none;
            }

            .recording-editor-panel-title {
                color: alpha(white, 0.45);
                font-size: 11px;
                font-weight: 600;
                margin-bottom: 6px;
                letter-spacing: 0.3px;
            }

            .recording-editor-panel-body {
                padding: 0;
                background: transparent;
                border: none;
                border-radius: 0;
            }

            /* ── Dropdowns ── */
            button.recording-editor-dropdown {
                min-height: 28px;
                border-radius: 6px;
                border: none;
                background: alpha(white, 0.06);
                background-image: none;
                color: #F1F1F3;
                padding: 0 8px;
                box-shadow: none;
                text-shadow: none;
            }

            button.recording-editor-dropdown:hover,
            button.recording-editor-dropdown:active {
                background: alpha(white, 0.10);
                background-image: none;
                box-shadow: none;
                outline: none;
            }

            .recording-editor-dropdown-label {
                color: #F1F1F3;
                font-size: 12px;
                font-weight: 500;
            }

            .recording-editor-dropdown-arrow {
                color: alpha(white, 0.45);
                font-size: 10px;
            }

            popover.recording-editor-dropdown-popover,
            popover.recording-editor-dropdown-popover > contents {
                background: transparent;
                border: none;
                box-shadow: none;
                padding: 0;
            }

            .recording-editor-dropdown-list {
                padding: 4px;
                border-radius: 8px;
                background: #1a1a1a;
                border: 1px solid alpha(white, 0.08);
                box-shadow: 0 8px 24px alpha(black, 0.45);
            }

            button.recording-editor-dropdown-item {
                min-height: 28px;
                padding: 0 8px;
                border-radius: 6px;
                border: none;
                background: transparent;
                color: #F1F1F3;
                box-shadow: none;
                font-size: 12px;
            }

            button.recording-editor-dropdown-item:hover {
                background: alpha(white, 0.06);
            }

            .recording-editor-label {
                color: alpha(white, 0.55);
                font-size: 12px;
            }

            /* ── Footer ── */
            .recording-editor-footer {
                padding: 6px 14px 8px 14px;
                background: transparent;
                border: none;
                border-radius: 0;
            }

            .recording-editor-estimate {
                color: alpha(white, 0.38);
                font-size: 11px;
            }

            .recording-editor-primary-button {
                min-width: 112px;
                background: #b05c38;
                color: white;
                border: none;
                border-radius: 6px;
                padding: 5px 14px;
                font-size: 12px;
                font-weight: 600;
            }

            .recording-editor-primary-button label {
                color: white;
                font-size: 12px;
                font-weight: 600;
            }

            .recording-editor-primary-button:hover {
                background: #c06540;
            }

            .recording-editor-primary-button:hover label {
                color: white;
            }

            .recording-editor-primary-button:disabled {
                opacity: 0.7;
                background: #b05c38;
                color: white;
            }

            .recording-editor-primary-button:disabled label {
                opacity: 1;
                color: white;
            }

            .recording-editor-secondary-button {
                min-width: 82px;
                background: alpha(white, 0.06);
                color: alpha(white, 0.78);
                border: none;
                border-radius: 6px;
                padding: 5px 14px;
                font-size: 12px;
                font-weight: 500;
            }

            .recording-editor-secondary-button label {
                color: alpha(white, 0.78);
            }

            .recording-editor-secondary-button:hover {
                background: alpha(white, 0.10);
            }

            .recording-editor-secondary-button:hover label {
                color: #ffffff;
            }

            .recording-editor-drop-banner {
                background: #1d1d1d;
                border-radius: 8px;
                border: 1px solid alpha(white, 0.10);
                padding: 8px 14px;
                margin: 46px 120px;
                box-shadow: 0 10px 28px alpha(black, 0.36);
            }

            .recording-editor-drop-label {
                color: alpha(white, 0.88);
                font-size: 12px;
                font-weight: 600;
            }

            /* ── Dialog ── */
            .recording-editor-dialog {
                background: #1e1e1e;
                border-radius: 12px;
                border: 1px solid alpha(white, 0.08);
                box-shadow: 0 12px 40px alpha(black, 0.55);
            }

            .recording-editor-dialog-root {
                background: transparent;
            }

            .recording-editor-dialog-bg {
                background: #1e1e1e;
                border-radius: 12px;
                border: 1px solid alpha(white, 0.08);
            }

            .recording-editor-dialog-title {
                color: #F1F1F3;
                font-size: 15px;
                font-weight: 700;
            }

            .recording-editor-dialog-body {
                color: alpha(white, 0.55);
                font-size: 12px;
            }

            /* ── Light theme overrides ── */
            .editor-theme-light.recording-editor-root {
                color: #1d2129;
                background: #ffffff;
            }

            .editor-theme-light.recording-editor-root entry {
                background-color: alpha(#111827, 0.04);
                color: #1d2129;
            }

            .editor-theme-light.recording-editor-root entry text {
                color: #1d2129;
                caret-color: #1d2129;
            }

            .editor-theme-light.recording-editor-root entry:focus {
                background-color: alpha(#111827, 0.08);
            }

            .editor-theme-light.recording-editor-root entry:disabled {
                background: alpha(#111827, 0.03);
                color: alpha(#1d2129, 0.42);
            }

            .editor-theme-light.recording-editor-root entry:disabled text {
                color: alpha(#1d2129, 0.42);
            }

            .editor-theme-light.recording-editor-root scale {
                color: #1d2129;
            }

            .editor-theme-light.recording-editor-root scale trough {
                background: alpha(#111827, 0.08);
            }

            .editor-theme-light.recording-editor-root scale highlight {
                background: #b05c38;
            }

            .editor-theme-light.recording-editor-root checkbutton {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light.recording-editor-root checkbutton check {
                background-color: alpha(#111827, 0.06);
                border: 1px solid alpha(#111827, 0.16);
                color: #1d2129;
            }

            .editor-theme-light.recording-editor-root checkbutton check:hover {
                border-color: alpha(#111827, 0.28);
                background-color: alpha(#111827, 0.10);
            }

            .editor-theme-light.recording-editor-root checkbutton:checked check,
            .editor-theme-light.recording-editor-root checkbutton check:checked,
            .editor-theme-light.recording-editor-root checkbutton.recording-editor-audio-choice check:checked {
                background-color: #b05c38;
                border-color: #b05c38;
                color: #ffffff;
            }

            .editor-theme-light.recording-editor-root checkbutton:disabled check {
                background-color: alpha(#111827, 0.03);
                border-color: alpha(#111827, 0.10);
                color: alpha(#1d2129, 0.35);
            }

            .editor-theme-light.recording-editor-root checkbutton label {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light.recording-editor-root checkbutton.recording-editor-audio-choice label {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light.recording-editor-root label {
                color: alpha(#1d2129, 0.85);
            }

            .editor-theme-light.recording-editor-root spinner {
                color: #1d2129;
            }

            .editor-theme-light.recording-editor-root scrollbar slider {
                background-color: alpha(#111827, 0.18);
            }

            .editor-theme-light.recording-editor-root scrollbar slider:hover {
                background-color: alpha(#111827, 0.28);
            }

            .editor-theme-light.recording-editor-root scrollbar slider:active {
                background-color: alpha(#111827, 0.35);
            }

            .editor-theme-light .recording-editor-window-controls {
                background: transparent;
            }

            .editor-theme-light .recording-editor-title {
                color: alpha(#1d2129, 0.72);
            }

            .editor-theme-light .recording-editor-traffic-btn {
                color: alpha(#111827, 0.65);
            }

            .editor-theme-light .recording-editor-traffic-btn:hover,
            .editor-theme-light .recording-editor-traffic-btn:active {
                background: alpha(#111827, 0.10);
                color: alpha(#111827, 0.65);
            }

            .editor-theme-light .recording-editor-traffic-btn:hover image,
            .editor-theme-light .recording-editor-traffic-btn:active image {
                color: alpha(#111827, 0.65);
            }

            .editor-theme-light .recording-editor-preview-frame {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-preview-workspace {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-video {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-empty-workspace {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-empty-icon {
                color: alpha(#1d2129, 0.34);
            }

            .editor-theme-light .recording-editor-empty-title {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-empty-hint {
                color: alpha(#1d2129, 0.52);
            }

            .editor-theme-light .recording-editor-empty-thumbnail-strip {
                background: alpha(#111827, 0.025);
                border-color: alpha(#111827, 0.10);
            }

            .editor-theme-light .recording-editor-drop-banner {
                background: #ffffff;
                border-color: alpha(#111827, 0.10);
                box-shadow: 0 10px 28px alpha(#111827, 0.16);
            }

            .editor-theme-light .recording-editor-drop-label {
                color: alpha(#1d2129, 0.82);
            }

            .editor-theme-light .recording-editor-drop-banner spinner {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-dim-badge {
                background: #b05c38;
                color: #ffffff;
                border-color: alpha(#111827, 0.10);
            }

            .editor-theme-light .recording-editor-dim-badge label,
            .editor-theme-light .recording-editor-dim-badge {
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-bottom-tools {
                background-color: #ffffff;
                border-top-color: alpha(#111827, 0.06);
            }

            .editor-theme-light .recording-editor-timeline-card {
                background: alpha(#111827, 0.04);
            }

            .editor-theme-light .recording-editor-play-button,
            .editor-theme-light .recording-editor-cut-button,
            .editor-theme-light .recording-editor-revert-button {
                background: alpha(#111827, 0.08);
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-play-button:hover,
            .editor-theme-light .recording-editor-cut-button:hover,
            .editor-theme-light .recording-editor-revert-button:hover,
            .editor-theme-light .recording-editor-cut-button-active {
                background: alpha(#111827, 0.14);
            }

            .editor-theme-light .recording-editor-cut-button-active {
                color: #b05c38;
            }

            .editor-theme-light .recording-editor-play-button image,
            .editor-theme-light .recording-editor-cut-button image,
            .editor-theme-light .recording-editor-revert-button image {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-cut-button-active image {
                color: #b05c38;
            }

            .editor-theme-light .recording-editor-thumbnail-strip {
                background: alpha(#111827, 0.06);
            }

            .editor-theme-light .recording-editor-thumbnail {
                background: alpha(#111827, 0.60);
                border-right-color: alpha(#ffffff, 0.12);
            }

            .editor-theme-light .recording-editor-trim-range {
                background: alpha(#b05c38, 0.15);
                border-top-color: #b05c38;
                border-bottom-color: #b05c38;
            }

            .editor-theme-light .recording-editor-trim-handle {
                background: #b05c38;
            }

            .editor-theme-light .recording-editor-time-label {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light .recording-editor-panel-title {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light button.recording-editor-dropdown {
                background: alpha(#111827, 0.06);
                color: #1d2129;
            }

            .editor-theme-light button.recording-editor-dropdown label {
                color: #1d2129;
            }

            .editor-theme-light button.recording-editor-dropdown:hover,
            .editor-theme-light button.recording-editor-dropdown:active {
                background: alpha(#111827, 0.10);
            }

            .editor-theme-light button.recording-editor-dropdown:hover label,
            .editor-theme-light button.recording-editor-dropdown:active label {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-dropdown-label {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-dropdown-arrow {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light .recording-editor-dropdown-list {
                background: #ffffff;
                border-color: alpha(#111827, 0.08);
                box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
            }

            .editor-theme-light .recording-editor-dropdown-list button {
                background: #ffffff;
            }

            .editor-theme-light button.recording-editor-dropdown-item {
                color: #1d2129;
                background: #ffffff;
            }

            .editor-theme-light button.recording-editor-dropdown-item:hover {
                background: alpha(#111827, 0.06);
            }

            .editor-theme-light .recording-editor-label {
                color: alpha(#1d2129, 0.55);
            }

            .editor-theme-light .recording-editor-estimate {
                color: alpha(#1d2129, 0.38);
            }

            .editor-theme-light .recording-editor-primary-button {
                background: #b05c38;
                color: white;
            }

            .editor-theme-light .recording-editor-primary-button label {
                color: white;
            }

            .editor-theme-light .recording-editor-primary-button:hover {
                background: #c06540;
            }

            .editor-theme-light .recording-editor-primary-button:hover label {
                color: white;
            }

            .editor-theme-light .recording-editor-primary-button:disabled {
                background: #b05c38;
                color: white;
            }

            .editor-theme-light .recording-editor-primary-button:disabled label {
                color: white;
            }

            .editor-theme-light .recording-editor-secondary-button {
                background: alpha(#111827, 0.06);
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light .recording-editor-secondary-button label {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light .recording-editor-secondary-button:hover {
                background: alpha(#111827, 0.10);
            }

            .editor-theme-light .recording-editor-secondary-button:hover label {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-dialog {
                background: #ffffff;
                border-color: alpha(#111827, 0.08);
                box-shadow: 0 12px 40px rgba(0, 0, 0, 0.15);
            }

            .editor-theme-light .recording-editor-dialog-bg {
                background: #ffffff;
                border-color: alpha(#111827, 0.08);
            }

            .editor-theme-light .recording-editor-dialog-title {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-dialog-body {
                color: alpha(#1d2129, 0.55);
            }
            ",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    }
}
