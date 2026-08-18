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
                min-height: 28px;
                padding: 2px 8px 2px 10px;
                background: transparent;
                border-bottom: 1px solid alpha(white, 0.06);
            }

            .recording-editor-window-controls.editor-toolbar {
                min-height: 28px;
            }

            .recording-editor-title {
                color: alpha(white, 0.62);
                font-size: 12px;
                font-weight: 500;
                padding: 0;
                margin: 0;
            }

            button.recording-editor-title-edit {
                min-width: 18px;
                min-height: 18px;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.38);
            }

            button.recording-editor-title-edit:hover {
                background: transparent;
                color: alpha(white, 0.78);
            }

            button.recording-editor-title-edit:disabled {
                opacity: 0.28;
            }

            .recording-editor-title-entry {
                min-height: 22px;
                min-width: 96px;
                max-width: 280px;
                padding: 0 6px;
                border: none;
                border-radius: 4px;
                background: alpha(white, 0.06);
                color: #F1F1F3;
                font-size: 12px;
            }

            .recording-editor-traffic-lights {
                min-height: 24px;
                margin: 0;
                padding: 0;
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

            .recording-editor-traffic-btn:hover {
                background: rgba(255, 255, 255, 0.08);
                background-image: none;
                color: rgba(236, 236, 238, 0.95);
                border-radius: 999px;
                border: none;
                box-shadow: none;
            }

            .recording-editor-traffic-btn:active {
                background: rgba(255, 255, 255, 0.08);
                background-image: none;
                color: rgba(236, 236, 238, 0.95);
                border: none;
                box-shadow: none;
            }

            .recording-editor-traffic-btn:focus {
                background: transparent;
                color: alpha(white, 0.65);
                border: none;
                box-shadow: none;
                outline: none;
            }

            .recording-editor-traffic-btn:hover:focus {
                background: rgba(255, 255, 255, 0.08);
                color: rgba(236, 236, 238, 0.95);
            }

            .recording-editor-traffic-btn:hover image,
            .recording-editor-traffic-btn:active image,
            .recording-editor-traffic-btn:hover:focus image {
                color: rgba(236, 236, 238, 0.95);
            }

            .recording-editor-traffic-close:hover,
            .recording-editor-traffic-close:hover:focus {
                background: #e81123;
                color: #ffffff;
            }

            .recording-editor-traffic-close:active {
                background: #c50f1f;
                color: #ffffff;
            }

            .recording-editor-traffic-close:hover image,
            .recording-editor-traffic-close:hover:focus image,
            .recording-editor-traffic-close:active image {
                color: #ffffff;
            }

            /* ── Preview ── */
            .recording-editor-preview-frame {
                background: #111111;
                min-height: 260px;
                padding: 28px 28px 0 28px;
                margin: 0;
            }

            .recording-editor-preview-workspace {
                padding: 0;
                margin: 0;
                background: #111111;
            }

            .recording-editor-preview-stage,
            .recording-editor-preview-stage > border {
                background: none;
                background-color: transparent;
                border: none;
                box-shadow: none;
                padding: 0;
                margin: 0;
                min-width: 0;
                min-height: 0;
            }

            .recording-editor-preview-canvas {
                background: #000000;
                border-radius: 4px;
                border: 1px solid alpha(white, 0.14);
            }

            .recording-editor-preview-clip {
                background: #000000;
                border-radius: 4px;
            }

            .recording-editor-player-bar {
                min-height: 28px;
                margin: 0 -28px 0 -28px;
                padding: 0 28px;
                background: #111111;
            }

            .recording-editor-player-clock {
                color: alpha(white, 0.6);
                font-size: 11px;
            }

            button.recording-editor-player-play {
                min-width: 28px;
                min-height: 28px;
                background: transparent;
                border: none;
                border-radius: 0;
                color: alpha(white, 0.82);
            }

            button.recording-editor-player-play:hover,
            button.recording-editor-player-play:hover:focus,
            button.recording-editor-player-play:active {
                background: transparent;
                color: #b05c38;
            }

            button.recording-editor-player-play image {
                color: alpha(white, 0.82);
            }

            button.recording-editor-player-play:hover image,
            button.recording-editor-player-play:hover:focus image,
            button.recording-editor-player-play:active image {
                color: #b05c38;
            }

            button.recording-editor-aspect-button {
                min-height: 24px;
                padding: 0 4px;
                background: transparent;
                border: none;
                color: alpha(white, 0.6);
                font-size: 11px;
            }

            button.recording-editor-aspect-button:hover {
                background: transparent;
                color: alpha(white, 0.88);
            }

            .recording-editor-aspect-label {
                color: inherit;
                font-size: 11px;
            }

            .recording-editor-video {
                background: #000000;
                border-radius: 4px;
                border: none;
                box-shadow: none;
                margin: 0;
                padding: 0;
            }

            .recording-editor-empty-workspace {
                min-height: 260px;
            }

            .recording-editor-empty-track-row {
                background: #2c2c2c;
                border-radius: 0;
            }

            .recording-editor-thumbnail-strip.recording-editor-empty-thumbnail-strip {
                background: transparent;
                border: none;
                border-radius: 0;
            }

            .recording-editor-empty-track-prompt {
                color: alpha(white, 0.42);
                font-size: 12px;
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
            /* A third, deeper black than the #111111 preview/top so the
               controls band reads as its own zone without a divider line.
               The timeline card and strip paint translucent white on top of
               it, so they lift back above it (~#141414 / ~#1d1d1d) and stay
               visibly distinct from both this band and the top. */
            .recording-editor-bottom-tools {
                padding: 0;
                margin: 0;
                background-color: #0c0c0c;
                border: none;
                border-radius: 0 0 10px 10px;
            }

            /* ── Timeline ── */
            .recording-editor-timeline {
                min-height: 48px;
                padding: 0;
                background: transparent;
                border-top: 1px solid alpha(white, 0.06);
            }

            .recording-editor-timeline-well {
                min-height: 14px;
                padding: 2px 8px 6px 0;
            }

            .recording-editor-timeline-scroll {
                min-height: 10px;
            }

            .recording-editor-timeline-shell,
            .recording-editor-timeline-card {
                border-radius: 0;
                background: transparent;
                border: none;
                padding: 0;
            }

            .recording-editor-transport {
                min-height: 28px;
                max-height: 28px;
                padding: 0;
                border-bottom: 1px solid alpha(white, 0.06);
            }

            button.recording-editor-tool-icon {
                min-width: 20px;
                min-height: 20px;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.62);
            }

            button.recording-editor-tool-icon:hover,
            button.recording-editor-tool-icon:hover:focus {
                background: transparent;
                color: #b05c38;
            }

            button.recording-editor-tool-icon image {
                color: inherit;
            }

            button.recording-editor-tool-icon:hover image,
            button.recording-editor-tool-icon-active,
            button.recording-editor-tool-icon-active image {
                color: #b05c38;
                background: transparent;
            }

            .recording-editor-timeline-scale-control {
                min-width: 132px;
            }

            .recording-editor-root scale.recording-editor-timeline-scale {
                min-width: 72px;
                min-height: 12px;
                padding: 0;
            }

            .recording-editor-root scale.recording-editor-timeline-scale trough {
                min-height: 3px;
            }

            .recording-editor-root scale.recording-editor-timeline-scale slider {
                min-width: 10px;
                min-height: 10px;
                margin: 0;
            }

            .recording-editor-timeline-board {
                min-height: 0;
            }

            .recording-editor-rail-divider {
                min-width: 1px;
                max-width: 1px;
                background: alpha(white, 0.12);
            }

            .recording-editor-track-row {
                min-height: 36px;
            }

            .recording-editor-track-header {
                min-width: 120px;
                padding: 0 8px 0 2px;
                border-right: none;
            }

            .recording-editor-transport-gutter {
                min-height: 28px;
                max-height: 28px;
            }

            .recording-editor-track-icon {
                color: alpha(white, 0.55);
            }

            button.recording-editor-rail-action {
                min-width: 18px;
                min-height: 18px;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.42);
            }

            button.recording-editor-rail-action:hover {
                background: transparent;
                color: alpha(white, 0.82);
            }

            button.recording-editor-rail-action:disabled {
                opacity: 0.22;
            }

            .recording-editor-track-body,
            .recording-editor-track-canvas,
            .recording-editor-trim-area,
            .recording-editor-waveform {
                position: relative;
                min-width: 0;
                overflow: hidden;
            }

            .recording-editor-track-faded {
                opacity: 0.3;
            }

            .recording-editor-track-locked {
                background-image: repeating-linear-gradient(
                    45deg,
                    alpha(white, 0.07),
                    alpha(white, 0.07) 2px,
                    transparent 2px,
                    transparent 6px
                );
                background-size: 6px 6px;
            }

            .editor-theme-light .recording-editor-track-locked {
                background-image: repeating-linear-gradient(
                    45deg,
                    alpha(#111827, 0.10),
                    alpha(#111827, 0.10) 2px,
                    transparent 2px,
                    transparent 6px
                );
            }

            .editor-theme-light .recording-editor-rail-divider {
                background: alpha(#111827, 0.14);
            }

            .editor-theme-light .recording-editor-track-icon {
                color: alpha(#1d2129, 0.55);
            }

            .editor-theme-light button.recording-editor-rail-action {
                color: alpha(#1d2129, 0.42);
            }

            .editor-theme-light button.recording-editor-rail-action:hover {
                color: alpha(#1d2129, 0.82);
                background: transparent;
            }

            .recording-editor-play-button,
            .recording-editor-cut-button,
            .recording-editor-revert-button {
                min-width: 32px;
                min-height: 32px;
                border-radius: 999px;
                background: alpha(white, 0.07);
                color: white;
                border: none;
            }

            .recording-editor-play-button-hero {
                min-width: 40px;
                min-height: 40px;
                background: #b05c38;
                color: #ffffff;
            }

            .recording-editor-play-button-hero image {
                color: #ffffff;
            }

            .recording-editor-play-button-hero:hover {
                background: #c06540;
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
                background: transparent;
                border-radius: 0;
                border: none;
                min-height: 64px;
                padding: 0;
                overflow: hidden;
            }

            .recording-editor-thumbnail {
                min-width: 0;
                min-height: 64px;
                background: #141414;
                border: none;
                border-radius: 0;
            }

            .recording-editor-thumbnail:first-child,
            .recording-editor-thumbnail:last-child {
                border-radius: 0;
                border: none;
            }

            .recording-editor-waveform,
            .recording-editor-zoom-track {
                padding: 0;
                background: alpha(white, 0.04);
                border-radius: 12px;
                min-height: 32px;
            }

            .recording-editor-waveform-image {
                border-radius: 12px;
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
            .recording-editor-inspector .recording-editor-panels {
                padding: 0;
                background: transparent;
                border: none;
            }

            .recording-editor-panels {
                padding: 0;
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
                margin-bottom: 2px;
                letter-spacing: 0.3px;
            }

            .recording-editor-convert-hint {
                color: alpha(#f0a07a, 0.85);
                font-size: 10px;
                font-weight: 500;
                margin-bottom: 6px;
            }

            .recording-editor-convert-only {
                opacity: 0.92;
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

            popover.recording-editor-aspect-popover,
            popover.recording-editor-aspect-popover > contents {
                background: transparent;
                border: none;
                outline: none;
                box-shadow: none;
                padding: 0;
                border-radius: 0;
            }

            .recording-editor-aspect-list {
                padding: 2px;
                border-radius: 0;
                border: none;
                outline: none;
                background: #1a1a1a;
                box-shadow: 0 8px 20px alpha(black, 0.4);
            }

            button.recording-editor-aspect-item {
                min-height: 26px;
                padding: 0 8px;
                border-radius: 0;
                border: none;
                outline: none;
            }

            .recording-editor-aspect-item-icon {
                color: alpha(white, 0.55);
            }

            .recording-editor-label {
                color: alpha(white, 0.55);
                font-size: 12px;
            }

            /* ── Footer ── */
            .recording-editor-inspector .recording-editor-footer {
                padding: 0;
            }

            .recording-editor-footer {
                padding: 0;
                background: transparent;
                border: none;
                border-radius: 0;
            }

            .recording-editor-inspector .recording-editor-primary-button,
            .recording-editor-inspector .recording-editor-secondary-button {
                min-width: 0;
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
                border-bottom-color: alpha(#111827, 0.08);
            }

            .editor-theme-light .recording-editor-title {
                color: alpha(#1d2129, 0.62);
            }

            .editor-theme-light button.recording-editor-title-edit {
                color: alpha(#1d2129, 0.38);
            }

            .editor-theme-light button.recording-editor-title-edit:hover {
                color: alpha(#1d2129, 0.78);
                background: transparent;
            }

            .editor-theme-light .recording-editor-title-entry {
                background: alpha(#111827, 0.06);
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-traffic-btn {
                color: alpha(#111827, 0.65);
            }

            .editor-theme-light .recording-editor-traffic-btn:hover {
                background: alpha(#111827, 0.10);
                color: alpha(#111827, 0.85);
            }

            .editor-theme-light .recording-editor-traffic-btn:active {
                background: alpha(#111827, 0.14);
                color: alpha(#111827, 0.85);
            }

            .editor-theme-light .recording-editor-traffic-btn:focus {
                background: transparent;
                color: alpha(#111827, 0.65);
                outline: none;
            }

            .editor-theme-light .recording-editor-traffic-btn:hover:focus {
                background: alpha(#111827, 0.10);
                color: alpha(#111827, 0.85);
            }

            .editor-theme-light .recording-editor-traffic-btn:hover image,
            .editor-theme-light .recording-editor-traffic-btn:active image,
            .editor-theme-light .recording-editor-traffic-btn:hover:focus image {
                color: alpha(#111827, 0.85);
            }

            .editor-theme-light .recording-editor-traffic-close:hover,
            .editor-theme-light .recording-editor-traffic-close:hover:focus {
                background: #e81123;
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-traffic-close:active {
                background: #c50f1f;
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-traffic-close:hover image,
            .editor-theme-light .recording-editor-traffic-close:hover:focus image,
            .editor-theme-light .recording-editor-traffic-close:active image {
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-preview-frame {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-preview-workspace {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-preview-stage,
            .editor-theme-light .recording-editor-preview-stage > border {
                background: none;
                background-color: transparent;
                border: none;
                box-shadow: none;
            }

            .editor-theme-light .recording-editor-preview-canvas {
                background: #111111;
                border-color: alpha(#111827, 0.16);
            }

            .editor-theme-light .recording-editor-preview-clip {
                background: #111111;
            }

            .editor-theme-light .recording-editor-player-bar {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-player-clock {
                color: alpha(#1d2129, 0.58);
            }

            .editor-theme-light button.recording-editor-player-play {
                background: transparent;
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light button.recording-editor-player-play:hover,
            .editor-theme-light button.recording-editor-player-play:hover:focus,
            .editor-theme-light button.recording-editor-player-play:active {
                background: transparent;
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-player-play image {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light button.recording-editor-player-play:hover image,
            .editor-theme-light button.recording-editor-player-play:hover:focus image,
            .editor-theme-light button.recording-editor-player-play:active image {
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-aspect-button {
                background: transparent;
                color: alpha(#1d2129, 0.58);
            }

            .editor-theme-light button.recording-editor-aspect-button:hover {
                background: transparent;
                color: alpha(#1d2129, 0.86);
            }

            .editor-theme-light .recording-editor-video {
                background: #111111;
            }

            .editor-theme-light .recording-editor-empty-workspace {
                background: #111111;
            }

            .editor-theme-light .recording-editor-empty-track-row {
                background: #e4e4e7;
            }

            .editor-theme-light .recording-editor-thumbnail-strip.recording-editor-empty-thumbnail-strip {
                background: transparent;
                border: none;
            }

            .editor-theme-light .recording-editor-empty-track-prompt {
                color: alpha(#1d2129, 0.45);
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

            /* Light-theme counterpart of the darker controls band: one step
               off the white top, still clearly lighter than the timeline card
               and strip that sit on it. */
            .editor-theme-light .recording-editor-bottom-tools {
                background-color: #f0f1f4;
            }

            .editor-theme-light button.recording-editor-tool-icon {
                background: transparent;
                color: alpha(#1d2129, 0.58);
            }

            .editor-theme-light button.recording-editor-tool-icon:hover,
            .editor-theme-light button.recording-editor-tool-icon:hover:focus,
            .editor-theme-light button.recording-editor-tool-icon-active,
            .editor-theme-light button.recording-editor-tool-icon-active image {
                background: transparent;
                color: #b05c38;
            }

            .editor-theme-light .recording-editor-timeline {
                border-top-color: alpha(#111827, 0.08);
            }

            .editor-theme-light .recording-editor-transport {
                border-bottom-color: alpha(#111827, 0.08);
            }

            .editor-theme-light .recording-editor-timeline-shell,
            .editor-theme-light .recording-editor-timeline-card {
                background: transparent;
                border: none;
            }

            .editor-theme-light .recording-editor-play-button-hero {
                background: #b05c38;
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-play-button-hero image {
                color: #ffffff;
            }

            .editor-theme-light .recording-editor-play-button-hero:hover {
                background: #c06540;
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
                background: transparent;
                border: none;
            }

            .editor-theme-light .recording-editor-thumbnail {
                background: #e4e4e6;
                border: none;
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

            .editor-theme-light .recording-editor-convert-hint {
                color: #b05c38;
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

            .editor-theme-light .recording-editor-aspect-list {
                background: #ffffff;
                border: none;
                box-shadow: 0 8px 20px rgba(0, 0, 0, 0.12);
            }

            .editor-theme-light .recording-editor-aspect-item-icon {
                color: alpha(#1d2129, 0.55);
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

            .recording-editor-workspace {
                min-height: 0;
            }

            .recording-editor-tool-rail,
            .recording-editor-tool-rail.editor-right-inspector,
            .recording-editor-tool-rail.recording-editor-inspector {
                min-width: 40px;
                max-width: 40px;
                width: 40px;
                padding: 10px 4px 8px 4px;
                background: alpha(white, 0.03);
                border-left: none;
                border-right: 1px solid alpha(white, 0.06);
            }

            .editor-theme-light .recording-editor-tool-rail,
            .editor-theme-light .recording-editor-tool-rail.editor-right-inspector,
            .editor-theme-light .recording-editor-tool-rail.recording-editor-inspector {
                background: alpha(#111827, 0.04);
                border-right-color: alpha(#111827, 0.08);
            }

            .recording-editor-tool-rail-group {
                padding: 0;
                background: transparent;
            }

            button.recording-editor-tool-rail-button {
                min-width: 32px;
                min-height: 32px;
                padding: 0;
                border-radius: 0;
                border: none;
                background: transparent;
                color: alpha(white, 0.55);
            }

            button.recording-editor-tool-rail-button:hover {
                background: transparent;
                color: alpha(white, 0.88);
            }

            button.recording-editor-tool-rail-add {
                background: transparent;
                color: alpha(white, 0.88);
            }

            button.recording-editor-tool-rail-active {
                color: #b05c38;
            }

            button.recording-editor-tool-rail-active image {
                color: #b05c38;
            }

            .recording-editor-media-library {
                min-width: 240px;
                max-width: 240px;
                padding: 10px 8px 8px 8px;
                background: transparent;
                border: none;
                border-right: 1px solid alpha(white, 0.06);
                border-radius: 0;
            }

            .recording-editor-media-header {
                min-height: 20px;
            }

            button.recording-editor-media-close {
                min-width: 20px;
                min-height: 20px;
                padding: 0;
                border: none;
                background: transparent;
                color: alpha(white, 0.45);
            }

            button.recording-editor-media-close:hover {
                color: #F1F1F3;
                background: transparent;
            }

            .recording-editor-media-tabs {
                background: transparent;
                border-radius: 0;
                margin: 0 -8px;
                padding: 0 8px 8px 8px;
                border-bottom: 1px solid alpha(white, 0.06);
            }

            button.recording-editor-media-tab {
                min-width: 0;
                min-height: 0;
                padding: 6px 0 4px 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.45);
            }

            button.recording-editor-media-tab image {
                color: inherit;
            }

            button.recording-editor-media-tab label {
                color: inherit;
                font-size: 10px;
                font-weight: 500;
            }

            button.recording-editor-media-tab:hover,
            button.recording-editor-media-tab:hover label,
            button.recording-editor-media-tab:hover image {
                color: alpha(white, 0.78);
                background: transparent;
            }

            button.recording-editor-media-tab-active,
            button.recording-editor-media-tab-active label,
            button.recording-editor-media-tab-active image {
                color: #b05c38;
                background: transparent;
            }

            button.recording-editor-media-upload {
                min-height: 88px;
                padding: 12px 8px;
                margin: 2px 0 4px 0;
                border: 1px dashed alpha(white, 0.22);
                border-radius: 10px;
                background: transparent;
                color: alpha(white, 0.48);
            }

            button.recording-editor-media-upload:hover,
            button.recording-editor-media-upload:hover:focus {
                background: transparent;
                border-color: #b05c38;
                color: #b05c38;
            }

            .recording-editor-media-upload-icon,
            .recording-editor-media-upload-title,
            .recording-editor-media-upload-hint {
                color: inherit;
            }

            .recording-editor-media-upload-title {
                font-size: 12px;
                font-weight: 600;
            }

            .recording-editor-media-upload-hint {
                font-size: 11px;
                font-weight: 400;
                opacity: 0.78;
            }

            .recording-editor-media-scroll,
            .recording-editor-media-list {
                background: transparent;
                border: none;
            }

            .recording-editor-media-row {
                min-height: 40px;
                padding: 4px 2px;
                background: transparent;
                border: none;
                border-radius: 0;
            }

            .recording-editor-media-thumb {
                min-width: 56px;
                min-height: 32px;
                border-radius: 3px;
                background: alpha(white, 0.04);
                overflow: hidden;
            }

            .recording-editor-media-kind-icon {
                color: alpha(white, 0.42);
            }

            .recording-editor-media-title {
                color: alpha(white, 0.78);
                font-size: 12px;
                font-weight: 500;
            }

            .recording-editor-media-meta,
            .recording-editor-media-empty {
                color: alpha(white, 0.38);
                font-size: 10px;
            }

            .editor-theme-light .recording-editor-media-library {
                background: transparent;
                border-right-color: alpha(#111827, 0.08);
            }

            .editor-theme-light .recording-editor-inspector,
            .editor-theme-light .recording-editor-inspector scrolledwindow,
            .editor-theme-light .recording-editor-inspector viewport,
            .editor-theme-light .recording-editor-inspector .editor-inspector-section,
            .editor-theme-light .recording-editor-inspector .recording-editor-panels,
            .editor-theme-light .recording-editor-inspector .recording-editor-panel,
            .editor-theme-light .recording-editor-inspector .recording-editor-panel-body {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-inspector {
                border-left-color: alpha(#111827, 0.08);
                border-right: none;
            }

            .editor-theme-light .recording-editor-inspector-toolbar {
                border-bottom-color: alpha(#111827, 0.08);
            }

            .editor-theme-light button.recording-editor-inspector-icon {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light button.recording-editor-inspector-icon:hover,
            .editor-theme-light button.recording-editor-inspector-icon:hover image {
                color: #1d2129;
            }

            .editor-theme-light button.recording-editor-media-close {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light button.recording-editor-media-close:hover {
                color: #1d2129;
            }

            .editor-theme-light .recording-editor-media-tabs {
                border-bottom-color: alpha(#111827, 0.08);
            }

            .editor-theme-light button.recording-editor-media-tab,
            .editor-theme-light button.recording-editor-media-tab label,
            .editor-theme-light button.recording-editor-media-tab image {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light button.recording-editor-media-tab:hover,
            .editor-theme-light button.recording-editor-media-tab:hover label,
            .editor-theme-light button.recording-editor-media-tab:hover image {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light button.recording-editor-media-tab-active,
            .editor-theme-light button.recording-editor-media-tab-active label,
            .editor-theme-light button.recording-editor-media-tab-active image {
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-media-upload {
                border-color: alpha(#111827, 0.20);
                color: alpha(#1d2129, 0.48);
                background: transparent;
            }

            .editor-theme-light button.recording-editor-media-upload:hover,
            .editor-theme-light button.recording-editor-media-upload:hover:focus {
                border-color: #b05c38;
                color: #b05c38;
                background: transparent;
            }

            .editor-theme-light .recording-editor-media-thumb {
                background: alpha(#111827, 0.04);
            }

            .editor-theme-light .recording-editor-media-kind-icon {
                color: alpha(#1d2129, 0.42);
            }

            .editor-theme-light .recording-editor-media-title {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light .recording-editor-media-meta,
            .editor-theme-light .recording-editor-media-empty {
                color: alpha(#1d2129, 0.42);
            }

            popover.recording-editor-tool-rail-popover,
            popover.recording-editor-tool-rail-popover > contents {
                background: #1a1a1a;
                border: 1px solid alpha(white, 0.08);
                border-radius: 10px;
                padding: 6px;
            }

            .recording-editor-inspector,
            .recording-editor-inspector scrolledwindow,
            .recording-editor-inspector viewport,
            .recording-editor-inspector .editor-inspector-section,
            .recording-editor-inspector .recording-editor-panels,
            .recording-editor-inspector .recording-editor-panel,
            .recording-editor-inspector .recording-editor-panel-body {
                background: #111111;
            }

            .recording-editor-inspector {
                min-width: 240px;
                max-width: 240px;
                padding: 4px 8px 8px 8px;
                border: none;
                border-left: 1px solid alpha(white, 0.06);
                border-radius: 0;
            }

            .recording-editor-inspector-toolbar,
            .recording-editor-inspector .editor-sidebar-actions {
                min-height: 24px;
                padding: 0 2px 2px 2px;
                margin: 0;
                border-bottom: 1px solid alpha(white, 0.06);
            }

            button.recording-editor-inspector-icon {
                min-width: 22px;
                min-height: 22px;
                padding: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.45);
            }

            button.recording-editor-inspector-icon:hover,
            button.recording-editor-inspector-icon:hover image {
                background: transparent;
                color: alpha(white, 0.88);
            }

            .recording-editor-inspector-toolbar .recording-editor-primary-button {
                min-width: 56px;
                min-height: 24px;
                padding: 2px 10px;
            }

            .recording-editor-preview-clip {
                background: #000000;
            }

            .recording-editor-ruler {
                min-height: 18px;
            }

            .recording-editor-stage {
                min-height: 0;
                background: #111111;
            }

            .recording-editor-timeline-dock {
                padding: 2px 12px 8px 12px;
                background: #111111;
                border-top: 1px solid alpha(white, 0.08);
            }

            .recording-editor-timeline-card {
                background: transparent;
                border: none;
                border-radius: 0;
                padding: 0;
            }

            .recording-editor-timeline-toolbar {
                min-height: 32px;
                padding: 2px 0 0 0;
            }

            button.recording-editor-timeline-tool {
                min-height: 24px;
                padding: 0 8px;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.62);
                font-size: 12px;
            }

            button.recording-editor-timeline-tool label,
            button.recording-editor-timeline-tool image {
                color: inherit;
            }

            button.recording-editor-timeline-tool:hover,
            button.recording-editor-timeline-tool-active {
                background: transparent;
                color: alpha(white, 0.92);
            }

            button.recording-editor-timeline-icon {
                min-width: 22px;
                min-height: 22px;
                padding: 0;
                border: none;
                background: transparent;
                color: alpha(white, 0.62);
            }

            button.recording-editor-timeline-icon:hover,
            button.recording-editor-timeline-play:hover {
                background: transparent;
                color: alpha(white, 0.94);
            }

            .recording-editor-timeline-clock {
                color: alpha(white, 0.78);
                font-size: 12px;
                font-weight: 500;
                font-variant-numeric: tabular-nums;
            }

            .recording-editor-timeline-zoom-row {
                min-width: 132px;
            }

            .recording-editor-root scale.recording-editor-timeline-zoom {
                min-width: 88px;
                min-height: 14px;
                padding: 0;
            }

            .recording-editor-root scale.recording-editor-timeline-zoom trough {
                min-height: 3px;
                background: alpha(white, 0.10);
            }

            .recording-editor-root scale.recording-editor-timeline-zoom highlight {
                background: #b05c38;
            }

            .recording-editor-root scale.recording-editor-timeline-zoom slider {
                min-width: 10px;
                min-height: 10px;
                background: #b05c38;
                border: none;
            }

            .recording-editor-card-board {
                min-height: 192px;
                padding: 0 0 10px 0;
            }

            .recording-editor-card-ruler {
                min-height: 36px;
            }

            .recording-editor-card-video-track {
                min-height: 80px;
            }

            .recording-editor-card-zoom-track {
                min-height: 56px;
            }

            .recording-editor-tool-sidebar {
                min-width: 288px;
                max-width: 288px;
                background: #111111;
                border-left: 1px solid alpha(white, 0.08);
            }

            .recording-editor-zoom-panel {
                min-width: 0;
                background: #111111;
            }

            .recording-editor-zoom-header {
                padding: 10px 14px 4px 14px;
            }

            .recording-editor-zoom-title {
                color: alpha(white, 0.92);
                font-size: 16px;
                font-weight: 600;
            }

            .recording-editor-zoom-kicker {
                color: alpha(white, 0.45);
                font-size: 11px;
                font-weight: 600;
                letter-spacing: 0.3px;
            }

            .recording-editor-zoom-scroll,
            .recording-editor-zoom-scroll > viewport,
            .recording-editor-zoom-scroll scrollbar {
                background: #111111;
                border: none;
            }

            .recording-editor-zoom-body {
                padding: 0 14px 8px 14px;
            }

            .recording-editor-zoom-mode {
                padding: 0;
                background: transparent;
                border: none;
            }

            button.recording-editor-zoom-mode-btn {
                min-height: 24px;
                padding: 0 8px 0 0;
                border: none;
                border-radius: 0;
                background: transparent;
                color: alpha(white, 0.52);
                font-size: 12px;
                font-weight: 500;
            }

            button.recording-editor-zoom-mode-btn label {
                color: inherit;
                font-size: 12px;
                font-weight: 500;
            }

            button.recording-editor-zoom-mode-btn:hover {
                color: alpha(white, 0.92);
                background: transparent;
            }

            button.recording-editor-zoom-mode-btn:checked,
            button.recording-editor-zoom-mode-btn:checked:hover {
                background: transparent;
                color: #b05c38;
            }

            button.recording-editor-zoom-mode-btn:checked label,
            button.recording-editor-zoom-mode-btn:checked:hover label {
                color: #b05c38;
            }

            button.recording-editor-zoom-mode-btn:disabled {
                opacity: 0.35;
            }

            .recording-editor-zoom-hint {
                margin-top: 4px;
                color: alpha(white, 0.38);
                font-size: 11px;
                line-height: 1.4;
            }

            .recording-editor-zoom-chips {
                margin-top: 12px;
            }

            button.recording-editor-zoom-chip {
                min-width: 0;
                min-height: 26px;
                padding: 0 4px;
                border-radius: 6px;
                border: none;
                background: alpha(white, 0.06);
                color: alpha(white, 0.62);
                font-size: 12px;
                font-weight: 500;
                font-variant-numeric: tabular-nums;
            }

            button.recording-editor-zoom-chip label {
                color: inherit;
                font-size: 12px;
                font-weight: 500;
            }

            button.recording-editor-zoom-chip:hover {
                background: alpha(white, 0.10);
                color: alpha(white, 0.92);
            }

            button.recording-editor-zoom-chip:active {
                background: alpha(white, 0.08);
            }

            button.recording-editor-zoom-chip-active,
            button.recording-editor-zoom-chip-active:hover {
                background: alpha(#b05c38, 0.18);
                color: #f0a07a;
            }

            button.recording-editor-zoom-chip-active label,
            button.recording-editor-zoom-chip-active:hover label {
                color: #f0a07a;
            }

            button.recording-editor-zoom-chip:disabled {
                opacity: 0.35;
            }

            .recording-editor-zoom-section-row {
                margin-top: 16px;
            }

            button.recording-editor-zoom-reset {
                min-height: 18px;
                padding: 0;
                background: transparent;
                color: #b05c38;
                font-size: 11px;
                font-weight: 500;
            }

            button.recording-editor-zoom-reset label {
                color: #b05c38;
                font-size: 11px;
                font-weight: 500;
            }

            button.recording-editor-zoom-reset:hover,
            button.recording-editor-zoom-reset:hover label {
                color: #f0a07a;
                background: transparent;
            }

            button.recording-editor-zoom-reset:disabled,
            button.recording-editor-zoom-reset:disabled label {
                opacity: 0.35;
            }

            .recording-editor-zoom-classic {
                margin-top: 8px;
                padding: 0;
                background: transparent;
            }

            .recording-editor-zoom-classic-label {
                color: alpha(white, 0.72);
                font-size: 12px;
            }

            switch.recording-editor-zoom-switch {
                min-width: 32px;
                min-height: 18px;
                background: alpha(white, 0.12);
                border: none;
                border-radius: 999px;
            }

            switch.recording-editor-zoom-switch slider {
                min-width: 14px;
                min-height: 14px;
                margin: 2px;
                background: #e8e4e0;
                border: none;
                box-shadow: none;
            }

            switch.recording-editor-zoom-switch:checked {
                background: #b05c38;
            }

            switch.recording-editor-zoom-switch:checked slider {
                background: #f4ece6;
            }

            switch.recording-editor-zoom-switch:disabled {
                opacity: 0.35;
            }

            .recording-editor-zoom-blur {
                margin-top: 16px;
                padding: 0;
                background: transparent;
                border: none;
            }

            .recording-editor-zoom-blur-value {
                color: alpha(white, 0.78);
                font-size: 12px;
                font-weight: 500;
                font-variant-numeric: tabular-nums;
            }

            .recording-editor-root scale.recording-editor-zoom-slider {
                min-height: 16px;
                padding: 0;
                margin: 0;
            }

            .recording-editor-root scale.recording-editor-zoom-slider trough {
                min-height: 3px;
                background: alpha(white, 0.10);
            }

            .recording-editor-root scale.recording-editor-zoom-slider highlight {
                background: #b05c38;
            }

            .recording-editor-root scale.recording-editor-zoom-slider slider {
                min-width: 10px;
                min-height: 10px;
                background: #b05c38;
                border: none;
            }

            button.recording-editor-zoom-delete {
                min-height: 24px;
                padding: 0;
                border: none;
                background: transparent;
                color: #F87171;
            }

            button.recording-editor-zoom-delete label,
            button.recording-editor-zoom-delete image {
                color: inherit;
                font-size: 12px;
                font-weight: 500;
            }

            button.recording-editor-zoom-delete:hover {
                background: transparent;
                color: #FCA5A5;
            }

            button.recording-editor-zoom-delete:disabled {
                opacity: 0.35;
            }

            .recording-editor-zoom-footer {
                padding: 6px 14px 12px 14px;
                border-top: 1px solid alpha(white, 0.06);
            }

            .editor-theme-light .recording-editor-stage,
            .editor-theme-light .recording-editor-timeline-dock {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-timeline-dock {
                border-top-color: alpha(#111827, 0.10);
            }

            .editor-theme-light .recording-editor-timeline-card {
                background: transparent;
                border: none;
            }

            .editor-theme-light button.recording-editor-timeline-tool,
            .editor-theme-light button.recording-editor-timeline-icon {
                color: alpha(#1d2129, 0.58);
            }

            .editor-theme-light button.recording-editor-timeline-tool:hover,
            .editor-theme-light button.recording-editor-timeline-tool-active,
            .editor-theme-light button.recording-editor-timeline-icon:hover {
                color: alpha(#1d2129, 0.88);
            }

            .editor-theme-light .recording-editor-timeline-clock {
                color: alpha(#1d2129, 0.72);
            }

            .editor-theme-light .recording-editor-tool-sidebar,
            .editor-theme-light .recording-editor-zoom-panel,
            .editor-theme-light .recording-editor-zoom-scroll,
            .editor-theme-light .recording-editor-zoom-scroll > viewport {
                background: #ffffff;
            }

            .editor-theme-light .recording-editor-tool-sidebar {
                border-left-color: alpha(#111827, 0.10);
            }

            .editor-theme-light .recording-editor-zoom-title {
                color: alpha(#1d2129, 0.92);
            }

            .editor-theme-light .recording-editor-zoom-kicker {
                color: alpha(#1d2129, 0.45);
            }

            .editor-theme-light .recording-editor-zoom-mode {
                background: transparent;
                border: none;
            }

            .editor-theme-light button.recording-editor-zoom-mode-btn {
                color: alpha(#1d2129, 0.52);
            }

            .editor-theme-light button.recording-editor-zoom-mode-btn:checked,
            .editor-theme-light button.recording-editor-zoom-mode-btn:checked:hover {
                background: transparent;
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-zoom-mode-btn:checked label,
            .editor-theme-light button.recording-editor-zoom-mode-btn:checked:hover label {
                color: #b05c38;
            }

            .editor-theme-light .recording-editor-zoom-hint {
                color: alpha(#1d2129, 0.42);
            }

            .editor-theme-light button.recording-editor-zoom-chip {
                background: alpha(#111827, 0.05);
                color: alpha(#1d2129, 0.62);
            }

            .editor-theme-light button.recording-editor-zoom-chip-active,
            .editor-theme-light button.recording-editor-zoom-chip-active:hover {
                background: alpha(#b05c38, 0.14);
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-zoom-chip-active label,
            .editor-theme-light button.recording-editor-zoom-chip-active:hover label {
                color: #b05c38;
            }

            .editor-theme-light button.recording-editor-zoom-reset,
            .editor-theme-light button.recording-editor-zoom-reset label {
                color: #b05c38;
            }

            .editor-theme-light .recording-editor-zoom-classic {
                background: transparent;
            }

            .editor-theme-light .recording-editor-zoom-classic-label {
                color: alpha(#1d2129, 0.72);
            }

            .editor-theme-light switch.recording-editor-zoom-switch {
                background: alpha(#111827, 0.16);
            }

            .editor-theme-light .recording-editor-zoom-blur {
                background: transparent;
                border: none;
            }

            .editor-theme-light .recording-editor-zoom-blur-value {
                color: alpha(#1d2129, 0.78);
            }

            .editor-theme-light .recording-editor-zoom-footer {
                border-top-color: alpha(#111827, 0.10);
            }

            .editor-theme-light button.recording-editor-zoom-delete {
                color: #DC2626;
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
