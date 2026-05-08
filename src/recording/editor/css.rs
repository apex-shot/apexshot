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
            }

            /* ── Strip native Adwaita/Ubuntu chrome from all descendants ── */
            .recording-editor-root button {
                background-image: none;
                text-shadow: none;
                box-shadow: none;
                -gtk-icon-shadow: none;
                border: 1px solid transparent;
                outline: none;
            }

            .recording-editor-root entry {
                background-image: none;
                box-shadow: none;
                border: 1px solid rgba(255, 255, 255, 0.11);
                border-radius: 8px;
                background-color: #000000;
                color: #f3f3f5;
                padding: 0 8px;
                min-height: 30px;
                outline: none;
            }

            .recording-editor-root entry text {
                color: #f7f8ff;
                font-size: 13px;
                background: transparent;
                caret-color: #f7f8ff;
            }

            .recording-editor-root entry:focus {
                border-color: rgba(114, 167, 255, 0.72);
            }

            .recording-editor-root entry:disabled {
                opacity: 0.52;
                background: rgba(0, 0, 0, 0.62);
                color: rgba(243, 243, 245, 0.62);
            }

            .recording-editor-root entry:disabled text {
                color: rgba(243, 243, 245, 0.62);
            }

            .recording-editor-root scale {
                color: #f3f3f5;
                min-height: 20px;
            }

            .recording-editor-root scale trough {
                min-height: 6px;
                border-radius: 999px;
                background: rgba(255, 255, 255, 0.08);
                border: 1px solid rgba(255, 255, 255, 0.08);
            }

            .recording-editor-root scale highlight {
                min-height: 6px;
                border-radius: 999px;
                background: #B05C38;
            }

            .recording-editor-root scale slider {
                min-width: 16px;
                min-height: 16px;
                border-radius: 999px;
                background: #f5f5f7;
                border: 1px solid rgba(0, 0, 0, 0.28);
                box-shadow: none;
            }

            .recording-editor-root checkbutton {
                padding: 2px 0;
                color: rgba(241, 241, 243, 0.88);
                background: transparent;
                border: none;
                box-shadow: none;
                text-shadow: none;
                -gtk-icon-shadow: none;
            }

            .recording-editor-root checkbutton check {
                min-width: 16px;
                min-height: 16px;
                border-radius: 999px;
                background: rgba(255, 255, 255, 0.03);
                border: 1px solid rgba(255, 255, 255, 0.16);
                box-shadow: none;
            }

            .recording-editor-root checkbutton:checked check {
                background: #B05C38;
                border-color: #B05C38;
                color: white;
            }

            .recording-editor-root checkbutton label {
                color: rgba(241, 241, 243, 0.88);
                font-size: 12px;
            }

            .recording-editor-root label {
                color: rgba(241, 241, 243, 0.88);
            }

            .recording-editor-root spinner {
                color: #f3f3f5;
            }

            .recording-editor-window-controls {
                min-height: 44px;
            }

            .recording-editor-title {
                color: rgba(245, 245, 247, 0.92);
                font-size: 13px;
                font-weight: 700;
            }

            .recording-editor-preview-frame {
                background: #000000;
                min-height: 260px;
                padding: 0;
                margin: 0;
            }

            .recording-editor-preview-workspace {
                padding: 0;
                margin: 0;
                background: #000000;
            }

            .recording-editor-video {
                background: #000000;
                border-radius: 0;
                border: none;
                box-shadow: none;
                margin: 0;
                padding: 0;
            }

            .recording-editor-bottom-tools {
                padding: 0;
                background-color: #141414;
                border-top: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 0 0 10px 10px;
            }

            .recording-editor-timeline {
                min-height: 72px;
                padding: 6px 14px;
                background: transparent;
            }

            .recording-editor-timeline-card {
                border-radius: 8px;
                background: rgba(20, 20, 20, 0.82);
                border: 1px solid rgba(255, 255, 255, 0.08);
                padding: 6px 8px;
            }

            .recording-editor-play-button {
                min-width: 36px;
                min-height: 36px;
                border-radius: 8px;
                background: rgba(255, 255, 255, 0.14);
                color: white;
                border: 1px solid rgba(255, 255, 255, 0.12);
                margin-right: 8px;
            }

            .recording-editor-play-button:hover {
                background: rgba(255, 255, 255, 0.22);
            }

            .recording-editor-thumbnail-strip {
                background: rgba(255, 255, 255, 0.08);
                border-radius: 4px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                min-height: 48px;
                padding: 2px;
            }

            .recording-editor-thumbnail {
                min-width: 48px;
                min-height: 32px;
                background: rgba(255, 255, 255, 0.75);
                border-right: 1px solid rgba(0, 0, 0, 0.18);
            }

            .recording-editor-trim-area {
                min-height: 48px;
            }

            .recording-editor-trim-range {
                background: rgba(255, 205, 0, 0.20);
                border-top: 4px solid #ffd400;
                border-bottom: 4px solid #ffd400;
            }

            .recording-editor-trim-handle {
                min-width: 10px;
                background: #ffd400;
                border-radius: 4px;
                border: 1px solid rgba(0, 0, 0, 0.35);
            }

            .recording-editor-time-label {
                color: rgba(255, 255, 255, 0.84);
                font-size: 11px;
            }

            .recording-editor-panels {
                padding: 10px 14px;
                background: #141414;
            }

            .recording-editor-panel {
                padding: 12px;
                border-radius: 6px;
                background: rgba(0, 0, 0, 0.42);
                border: 1px solid rgba(255, 255, 255, 0.08);
            }

            button.recording-editor-dropdown {
                min-height: 32px;
                border-radius: 6px;
                border: 1px solid rgba(255, 255, 255, 0.11);
                background: #000000;
                background-image: none;
                color: #f3f3f5;
                box-shadow: none;
                text-shadow: none;
            }

            button.recording-editor-dropdown:hover,
            button.recording-editor-dropdown:active,
            button.recording-editor-dropdown:focus {
                background: #000000;
                background-image: none;
                border-color: rgba(255, 255, 255, 0.18);
                box-shadow: none;
                outline: none;
            }

            .recording-editor-dropdown-label {
                color: #f3f3f5;
                font-size: 13px;
                font-weight: 500;
            }

            .recording-editor-dropdown-arrow {
                color: rgba(243, 243, 245, 0.76);
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
                padding: 8px;
                border-radius: 12px;
                background-image: linear-gradient(to bottom,
                    rgba(28, 28, 34, 0.98),
                    rgba(18, 18, 23, 0.98));
                border: 1px solid rgba(255, 255, 255, 0.10);
                box-shadow:
                    0 14px 32px rgba(0, 0, 0, 0.55),
                    inset 0 1px 0 rgba(255, 255, 255, 0.04);
            }

            button.recording-editor-dropdown-item {
                min-width: 190px;
                min-height: 30px;
                padding: 0 12px;
                border-radius: 8px;
                border: 1px solid transparent;
                background: rgba(255, 255, 255, 0.01);
                color: #f3f3f5;
                box-shadow: none;
            }

            button.recording-editor-dropdown-item:hover {
                background: rgba(255, 255, 255, 0.07);
                border-color: rgba(255, 255, 255, 0.09);
            }

            .recording-editor-panel-title,
            .recording-editor-label {
                color: rgba(255, 255, 255, 0.86);
                font-size: 12px;
            }

            .recording-editor-footer {
                padding: 8px 14px 12px 14px;
                background: #141414;
                border-radius: 0 0 10px 10px;
            }

            .recording-editor-estimate {
                color: rgba(255, 255, 255, 0.58);
                font-size: 12px;
            }

            .recording-editor-primary-button {
                min-width: 112px;
                background: #f5f5f7;
                color: #050505;
                border: 1px solid #f5f5f7;
                border-radius: 5px;
                padding: 5px 12px;
            }

            .recording-editor-secondary-button {
                min-width: 82px;
                background: transparent;
                color: rgba(255, 255, 255, 0.88);
                border: 1px solid transparent;
                border-radius: 5px;
                padding: 5px 12px;
            }

            .recording-editor-secondary-button:hover {
                background: #1a1a1d;
                border-color: rgba(255, 255, 255, 0.09);
            }
            ",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
}
