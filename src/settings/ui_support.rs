use gtk4::gdk;
use gtk4::{prelude::*, Button, CssProvider};

const SETTINGS_CSS: &str = r#"
            /* Main settings window transparency for rounded corners */
            window.editor-window,
            .editor-window {
                background-color: transparent;
                border: none;
                border-radius: 10px;
            }

            .editor-root {
                border-radius: 10px;
                background-color: #141414;
                border: 1px solid alpha(white, 0.10);
                color: #F1F1F3;
            }

            .editor-toolbar {
                padding: 8px 12px;
                background-color: #141414;
                border-radius: 10px 10px 0 0;
            }

            /* ── Scrollbar ── */
            .editor-root scrollbar slider {
                background-color: alpha(white, 0.18);
                border-radius: 999px;
                min-width: 4px;
                min-height: 4px;
                border: none;
            }

            .editor-root scrollbar slider:hover {
                background-color: alpha(white, 0.28);
            }

            .editor-root scrollbar slider:active {
                background-color: alpha(white, 0.35);
            }

            .editor-root scrollbar trough {
                background: transparent;
                border: none;
            }

            .editor-root.editor-theme-light scrollbar slider {
                background-color: alpha(#111827, 0.18);
            }

            .editor-root.editor-theme-light scrollbar slider:hover {
                background-color: alpha(#111827, 0.30);
            }

            .editor-root.editor-theme-light scrollbar slider:active {
                background-color: alpha(#111827, 0.40);
            }

            .settings-window-controls {
                min-height: 0;
                padding: 0;
                background: rgba(20, 20, 20, 0.94);
                border-bottom: 1px solid rgba(255, 255, 255, 0.08);
            }

            .settings-window-controls button.recording-editor-traffic-btn {
                min-width: 24px;
                min-height: 24px;
                padding: 0;
                margin: 0;
                border-radius: 999px;
                background-color: transparent;
                background-image: none;
                color: rgba(255, 255, 255, 0.65);
                border: none;
                box-shadow: none;
            }

            .settings-window-controls button.recording-editor-traffic-btn image {
                -gtk-icon-size: 14px;
            }

            .settings-window-controls button.recording-editor-traffic-btn:hover,
            .settings-window-controls button.recording-editor-traffic-btn:active,
            .settings-window-controls button.recording-editor-traffic-btn:focus {
                background-color: rgba(255, 255, 255, 0.10);
                background-image: none;
                color: #ffffff;
                border-radius: 999px;
                border: none;
                box-shadow: none;
            }

            .settings-window-controls button.recording-editor-traffic-btn:hover image,
            .settings-window-controls button.recording-editor-traffic-btn:active image,
            .settings-window-controls button.recording-editor-traffic-btn:focus image {
                color: #ffffff;
            }

            .editor-toolbar-left,
            .editor-toolbar-right {
                min-height: 32px;
            }

            .editor-traffic-lights {
                margin-left: 0;
                margin-right: 10px;
            }

            .editor-root.editor-theme-light {
                background-color: #f6f7fb;
                color: #1d2129;
                border-color: alpha(#111827, 0.10);
            }

            .editor-root.editor-theme-light .editor-toolbar {
                background-color: #f6f7fb;
            }

            .editor-root.editor-theme-light .settings-window-controls {
                background: #f0f1f5;
                border-bottom: 1px solid alpha(#111827, 0.06);
            }

            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn {
                color: alpha(#111827, 0.55);
                background-color: transparent;
                background-image: none;
                border: none;
                box-shadow: none;
            }

            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:hover,
            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:active,
            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:focus {
                background-color: alpha(#111827, 0.08);
                color: #1d2129;
                border: none;
                border-radius: 999px;
                box-shadow: none;
            }

            .editor-root.editor-reduced-transparency {
                background-color: #111318;
            }

            .editor-root.editor-reduced-transparency.editor-theme-light {
                background-color: #ffffff;
            }

            .settings-sidebar-wrapper {
                background-color: alpha(black, 0.25);
                border-right: 1px solid alpha(white, 0.06);
                min-width: 170px;
            }

            .settings-sidebar {
                padding: 10px 8px;
                background-color: transparent;
                border: none;
            }

            .settings-nav-item {
                min-height: 24px;
                padding: 5px 10px;
                border-radius: 5px;
                margin-bottom: 1px;
            }

            .settings-nav-item-hover {
                background-color: alpha(white, 0.06);
            }

            .settings-nav-item-selected {
                background-color: alpha(white, 0.10);
                box-shadow: none;
            }

            .settings-nav-icon {
                opacity: 0.72;
            }

            .settings-nav-icon-selected {
                opacity: 1;
                color: #e8764a;
            }

            .settings-nav-label {
                font-size: 12px;
                font-weight: 500;
                margin-top: 0;
                opacity: 0.78;
            }

            .settings-nav-label-selected {
                opacity: 1;
                color: #ffffff;
            }

            .settings-nav-icon-hover,
            .settings-nav-label-hover {
                color: white;
                opacity: 1;
            }

            button.settings-primary-btn {
                background-image: none;
                background-color: #b05c38;
                border: none;
                border-radius: 6px;
                padding: 4px 16px;
                font-size: 12px;
                font-weight: 600;
                color: white;
                min-height: 24px;
                box-shadow: none;
            }

            button.settings-primary-btn:hover {
                background-image: none;
                background-color: #c06540;
                box-shadow: none;
            }

            .editor-root.editor-theme-light .settings-sidebar-wrapper {
                border-right-color: alpha(#111827, 0.06);
                background-color: alpha(#111827, 0.04);
            }

            .editor-root.editor-theme-light .settings-sidebar {
                background-color: transparent;
                border: none;
            }

            .editor-root.editor-theme-light .settings-nav-item-hover {
                background-color: alpha(#111827, 0.06);
            }

            .editor-root.editor-theme-light .settings-nav-item-selected {
                background-color: alpha(#111827, 0.08);
                box-shadow: none;
            }

            .editor-root.editor-theme-light .settings-nav-icon {
                color: #1d2129;
                opacity: 0.62;
            }

            .editor-root.editor-theme-light .settings-nav-icon-selected {
                color: #b05c38;
                opacity: 1;
            }

            .editor-root.editor-theme-light .settings-nav-label {
                color: #1d2129;
                opacity: 0.72;
            }

            .editor-root.editor-theme-light .settings-nav-label-selected {
                color: #1d2129;
                opacity: 1;
            }

            .editor-root.editor-theme-light .settings-nav-icon-hover,
            .editor-root.editor-theme-light .settings-nav-label-hover {
                color: #1d2129;
                opacity: 1;
            }

            .editor-root.editor-theme-light button.settings-primary-btn {
                background-color: #b05c38;
                border-color: #9a4c2c;
                color: white;
            }

            .editor-root.editor-theme-light button.settings-primary-btn:hover {
                background-color: #c06540;
            }

            .settings-toast {
                padding: 8px 20px;
                border-radius: 8px;
                border: none;
                background-color: alpha(white, 0.08);
                color: rgba(255,255,255,0.88);
                font-size: 12px;
                font-weight: 500;
                box-shadow: none;
            }

            .settings-toast-success {
                background-color: alpha(#b05c38, 0.18);
                color: #e8a07a;
            }

            .settings-toast-error {
                background-color: alpha(#cf433c, 0.18);
                color: #e8807a;
            }

            .editor-root.editor-theme-light .settings-toast {
                background-color: alpha(#111827, 0.06);
                color: #17202a;
                border: none;
            }

            .editor-root.editor-theme-light .settings-toast-success {
                background-color: alpha(#b05c38, 0.12);
                color: #8a4422;
            }

            .editor-root.editor-theme-light .settings-toast-error {
                background-color: alpha(#cf433c, 0.12);
                color: #8a2822;
            }

            .settings-page-title {
                font-size: 14px;
                font-weight: 600;
                letter-spacing: -0.1px;
                opacity: 0.85;
            }

            .settings-group-title {
                font-size: 11px;
                font-weight: 600;
                opacity: 0.45;
                margin-top: 18px;
                margin-bottom: 4px;
                letter-spacing: 0.5px;
            }

            .settings-sub-option {
                font-size: 12px;
                opacity: 0.64;
            }

            .settings-scale-caption {
                font-size: 13px;
                font-weight: 700;
                opacity: 0.94;
            }

            .settings-table-header {
                font-size: 13px;
                font-weight: 700;
            }

            .settings-table-frame {
                border-radius: 10px;
                border: none;
                background-color: alpha(white, 0.04);
            }

            .editor-root.editor-theme-light .settings-table-frame {
                border: none;
                background-color: alpha(#111827, 0.04);
            }

            .settings-table-row {
                padding: 8px 14px;
                border-bottom: 1px solid alpha(white, 0.04);
            }

            .settings-table-row:last-child {
                border-bottom: none;
            }

            .settings-table-row-muted {
                background-color: transparent;
            }

            .editor-root.editor-theme-light .settings-table-row-muted {
                background-color: transparent;
            }

            .editor-canvas-frame {
                border-radius: 0;
                border: none;
                background-color: transparent;
                padding: 0;
            }

            .editor-footer {
                padding: 6px 12px;
                background-color: #141414;
                border-top: 1px solid alpha(white, 0.08);
                border-radius: 0 0 10px 10px;
            }

            .editor-root.editor-theme-light .editor-footer {
                background-color: #f6f7fb;
                border-top-color: alpha(#111827, 0.08);
            }

            .settings-select {
                min-width: 180px;
            }

            /* ── Dark-mode native widget overrides ── */
            /* Prevent system light-theme from bleeding into native GTK widgets */

            .editor-root entry {
                background-image: none;
                background-color: alpha(white, 0.06);
                color: #F1F1F3;
                border: none;
                border-radius: 6px;
            }

            .editor-root entry text {
                color: #F1F1F3;
            }

            .editor-root entry:focus {
                border-color: alpha(#b05c38, 0.55);
            }

            .editor-root entry:disabled {
                background-color: alpha(white, 0.03);
                color: alpha(white, 0.42);
                border-color: alpha(white, 0.08);
            }

            .editor-root entry:disabled text {
                color: alpha(white, 0.42);
            }

            .editor-root checkbutton check {
                background-image: none;
                background-color: alpha(white, 0.06);
                border: 1px solid alpha(white, 0.16);
                color: #ffffff;
            }

            .editor-root checkbutton check:hover {
                border-color: alpha(white, 0.28);
                background-color: alpha(white, 0.10);
            }

            .editor-root checkbutton:checked check {
                background-color: #b05c38;
                border-color: #b05c38;
                color: #ffffff;
            }

            .editor-root checkbutton:disabled check {
                background-color: alpha(white, 0.03);
                border-color: alpha(white, 0.10);
                color: alpha(white, 0.35);
            }

            .editor-root .settings-select {
                background-image: none;
                background-color: alpha(white, 0.06);
                color: #F1F1F3;
                border: none;
            }

            .editor-root .settings-select button {
                background-image: none;
                background-color: transparent;
                color: #F1F1F3;
                border: none;
            }

            .editor-root .settings-select:focus {
                border-color: alpha(#b05c38, 0.55);
                outline: none;
            }

            .editor-root .settings-select:disabled {
                background-color: alpha(white, 0.03);
                color: alpha(white, 0.42);
                border-color: alpha(white, 0.08);
            }

            .editor-root .settings-select:disabled button {
                color: alpha(white, 0.42);
            }

            .editor-root .settings-select popover,
            .editor-root .settings-select popover contents {
                background-color: #141414;
                color: #F1F1F3;
                border: 1px solid alpha(white, 0.12);
                border-radius: 8px;
            }

            .editor-root .settings-select popover list,
            .editor-root .settings-select popover listview,
            .editor-root .settings-select popover row,
            .editor-root .settings-select popover modelbutton {
                background-color: transparent;
                color: #F1F1F3;
            }

            .editor-root .settings-select popover row:hover,
            .editor-root .settings-select popover modelbutton:hover {
                background-color: alpha(white, 0.08);
                color: #F1F1F3;
            }

            .editor-root scale trough {
                min-height: 4px;
                background-color: alpha(white, 0.08);
                border: none;
                border-radius: 999px;
            }

            .editor-root scale highlight {
                min-height: 4px;
                background-color: #b05c38;
                border-radius: 999px;
            }

            .editor-root scale slider {
                min-width: 14px;
                min-height: 14px;
                background: #F1F1F3;
                border: none;
                border-radius: 999px;
                box-shadow: none;
            }

            .editor-root scale:disabled trough {
                background-color: alpha(white, 0.04);
            }

            .editor-root scale:disabled highlight {
                background-color: alpha(#b05c38, 0.42);
            }

            .editor-root scale:disabled slider {
                background: alpha(#F1F1F3, 0.42);
            }

            .editor-root button {
                background-image: none;
                background-color: alpha(white, 0.06);
                color: #F1F1F3;
                border: none;
                border-radius: 6px;
            }

            .editor-root button:hover {
                background-color: alpha(white, 0.10);
            }

            .editor-root button:active {
                background-color: alpha(white, 0.14);
            }

            .editor-root button:disabled {
                background-color: alpha(white, 0.03);
                color: alpha(white, 0.42);
            }

            /* ── Light-mode native widget overrides ── */

            .editor-root.editor-theme-light entry,
            .editor-root.editor-theme-light .settings-select,
            .editor-root.editor-theme-light .settings-select button {
                background-image: none;
                background-color: #ffffff;
                color: #17202a;
                border-color: alpha(#111827, 0.18);
            }

            .editor-root.editor-theme-light entry:focus,
            .editor-root.editor-theme-light .settings-select:focus,
            .editor-root.editor-theme-light .settings-select button:focus {
                border-color: alpha(#b05c38, 0.55);
                outline: none;
            }

            .editor-root.editor-theme-light entry:disabled,
            .editor-root.editor-theme-light .settings-select:disabled,
            .editor-root.editor-theme-light .settings-select button:disabled {
                background-color: alpha(#111827, 0.04);
                color: alpha(#17202a, 0.48);
                border-color: alpha(#111827, 0.10);
            }

            .editor-root.editor-theme-light checkbutton check {
                background-image: none;
                background-color: #ffffff;
                border: 1px solid alpha(#111827, 0.28);
                color: #ffffff;
            }

            .editor-root.editor-theme-light checkbutton check:hover {
                border-color: alpha(#111827, 0.42);
                background-color: #f8fafc;
            }

            .editor-root.editor-theme-light checkbutton:checked check {
                background-color: #b05c38;
                border-color: #9a4c2c;
                color: #ffffff;
            }

            .editor-root.editor-theme-light checkbutton:disabled check {
                background-color: alpha(#111827, 0.04);
                border-color: alpha(#111827, 0.12);
                color: alpha(#17202a, 0.35);
            }

            .editor-root.editor-theme-light .settings-select popover,
            .editor-root.editor-theme-light .settings-select popover contents {
                background-color: #f6f7fb;
                color: #17202a;
                border-color: alpha(#111827, 0.12);
            }

            .editor-root.editor-theme-light .settings-select popover list,
            .editor-root.editor-theme-light .settings-select popover listview,
            .editor-root.editor-theme-light .settings-select popover row,
            .editor-root.editor-theme-light .settings-select popover modelbutton {
                background-color: #ffffff;
                color: #17202a;
            }

            .editor-root.editor-theme-light .settings-select popover row:hover,
            .editor-root.editor-theme-light .settings-select popover modelbutton:hover {
                background-color: alpha(#111827, 0.06);
                color: #17202a;
            }

            .editor-root.editor-theme-light entry text {
                color: #17202a;
            }

            .editor-root.editor-theme-light entry:disabled text {
                color: alpha(#17202a, 0.48);
            }

            .editor-root.editor-theme-light scale trough {
                min-height: 4px;
                background-color: alpha(#111827, 0.08);
                border: none;
                border-radius: 999px;
            }

            .editor-root.editor-theme-light scale highlight {
                min-height: 4px;
                background-color: #b05c38;
                border-radius: 999px;
            }

            .editor-root.editor-theme-light scale slider {
                min-width: 14px;
                min-height: 14px;
                background: #ffffff;
                border: none;
                border-radius: 999px;
                box-shadow: none;
            }

            .editor-root.editor-theme-light scale:disabled trough {
                background-color: alpha(#111827, 0.04);
            }

            .editor-root.editor-theme-light scale:disabled highlight {
                background-color: alpha(#b05c38, 0.42);
            }

            .editor-root.editor-theme-light scale:disabled slider {
                background: alpha(#ffffff, 0.62);
            }

            .editor-root.editor-theme-light button {
                background-image: none;
                background-color: alpha(#111827, 0.04);
                color: #17202a;
                border: none;
            }

            .editor-root.editor-theme-light button:hover {
                background-color: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light button:active {
                background-color: alpha(#111827, 0.10);
            }

            .editor-root.editor-theme-light button:disabled {
                background-color: alpha(#111827, 0.02);
                color: alpha(#17202a, 0.48);
            }

            .editor-root.editor-theme-light .recording-tab-switcher {
                background-color: alpha(#111827, 0.04);
                border-color: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light .recording-tab-button {
                color: alpha(#111827, 0.6);
            }

            .editor-root.editor-theme-light .recording-tab-button:hover {
                color: #17202a;
            }

            .editor-root.editor-theme-light .settings-action-button {
                background-color: alpha(#111827, 0.06);
                border-color: alpha(#111827, 0.12);
                color: #17202a;
            }

            .editor-root.editor-theme-light .settings-action-button:hover {
                background-color: alpha(#111827, 0.10);
            }

            .editor-root.editor-theme-light .mode-preview-box {
                background-color: alpha(#111827, 0.04);
            }

            .editor-root.editor-theme-light .mode-preview-box.active {
                border-color: #b05c38;
                background-color: alpha(#b05c38, 0.08);
                box-shadow: 0 4px 12px alpha(black, 0.12);
            }

            .editor-root.editor-theme-light .shortcuts-row-zebra {
                background-color: alpha(#111827, 0.04);
            }

            .recording-tab-switcher {
                background-color: alpha(white, 0.04);
                border-radius: 9px;
                padding: 4px;
                border: 1px solid alpha(white, 0.08);
            }

            .recording-tab-button {
                min-width: 90px;
                min-height: 28px;
                background: transparent;
                border: none;
                border-radius: 6px;
                color: alpha(white, 0.6);
                font-size: 13px;
                font-weight: 500;
                box-shadow: none;
                transition: all 0.2s;
            }

            .recording-tab-button:hover {
                color: white;
            }

            .recording-tab-button.active {
                background-color: #b05c38;
                color: white;
                box-shadow: 0 1px 3px alpha(black, 0.2);
            }

            .settings-action-button {
                min-height: 24px;
                padding: 2px 10px;
                background-color: alpha(white, 0.08);
                border-radius: 6px;
                border: 1px solid alpha(white, 0.1);
                font-size: 11px;
                color: white;
            }

            .settings-action-button:hover {
                background-color: alpha(white, 0.12);
            }

            .settings-sub-option-hint {
                font-size: 11px;
                opacity: 0.64;
                line-height: 1.4;
            }

            .mode-preview-box {
                background-color: alpha(white, 0.04);
                border-radius: 10px;
                border: 2px solid transparent;
                transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
            }

            .mode-preview-box.active {
                border-color: #b05c38;
                background-color: alpha(#b05c38, 0.08);
                box-shadow: 0 4px 12px alpha(black, 0.3);
            }

            .mode-icon-check {
                opacity: 0;
                min-width: 0;
                min-height: 0;
            }

            .selection-mode-radio {
                margin-right: 8px;
            }

            .shortcuts-header-title {
                font-weight: bold;
                font-size: 15px;
            }

            .shortcuts-row-zebra {
                background-color: alpha(white, 0.04);
            }

            .shortcuts-label {
                font-size: 13px;
                opacity: 0.9;
            }

            .shortcuts-record-btn {
                background-color: alpha(white, 0.06);
                border: 1px solid alpha(white, 0.1);
                border-radius: 8px;
                color: alpha(white, 0.86);
                font-family: monospace;
                transition: all 0.2s;
            }

            .shortcuts-record-btn:hover {
                background-color: alpha(white, 0.12);
                border-color: alpha(white, 0.2);
            }

            .shortcuts-record-btn:active {
                background-color: alpha(white, 0.2);
            }

            .editor-root.editor-theme-light .shortcuts-record-btn {
                background-image: none;
                background-color: #ffffff;
                border-color: alpha(#111827, 0.18);
                color: #17202a;
            }

            .editor-root.editor-theme-light .shortcuts-record-btn:hover {
                background-color: #f8fafc;
                border-color: alpha(#111827, 0.28);
                color: #17202a;
            }

            .editor-root.editor-theme-light .shortcuts-record-btn:active {
                background-color: alpha(#b05c38, 0.10);
                border-color: alpha(#b05c38, 0.35);
            }

            .shortcuts-tip {
                font-size: 13px;
                line-height: 1.55;
            }

            .shortcut-capture-dialog {
                background: #1a1a1a;
                border-radius: 12px;
                border: 1px solid alpha(white, 0.08);
            }

            .shortcut-capture-title {
                font-size: 16px;
                font-weight: 600;
                color: white;
            }

            .shortcut-capture-subtitle {
                font-size: 13px;
                color: rgba(255,255,255,0.78);
                line-height: 1.35;
            }

            .shortcut-capture-hint {
                font-size: 11px;
                color: rgba(255,255,255,0.45);
                line-height: 1.35;
                margin-top: 10px;
            }

            .shortcut-capture-listening-icon {
                font-size: 56px;
                color: #e8764a;
                margin-top: 14px;
                margin-bottom: 10px;
            }

            .shortcut-capture-keycaps-row {
                margin-top: 14px;
            }

            .shortcut-capture-keycap {
                background: alpha(white, 0.08);
                color: white;
                border-radius: 6px;
                padding: 6px 12px;
                font-size: 13px;
                font-weight: 600;
            }

            .shortcut-capture-plus {
                color: rgba(255,255,255,0.45);
                font-size: 16px;
                font-weight: 700;
                margin-top: 5px;
            }

            .shortcut-capture-cleared-label {
                color: rgba(255,255,255,0.64);
                font-size: 13px;
            }

            .shortcut-capture-primary-btn {
                background: #b05c38;
                color: white;
                border-radius: 6px;
                padding: 6px 16px;
                font-weight: 600;
                font-size: 12px;
                border: none;
            }

            .shortcut-capture-primary-btn:hover {
                background: #c06540;
            }

            .shortcut-capture-primary-btn:disabled {
                background: alpha(white, 0.06);
                color: rgba(255,255,255,0.35);
            }

            .shortcut-capture-secondary-btn {
                background: alpha(white, 0.06);
                color: white;
                border-radius: 6px;
                padding: 6px 16px;
                font-weight: 600;
                font-size: 12px;
                border: none;
            }

            .shortcut-capture-secondary-btn:hover {
                background: alpha(white, 0.10);
            }

            .secondary-settings-button {
                background: alpha(white, 0.06);
                border: none;
                border-radius: 6px;
                padding: 4px 12px;
                font-size: 12px;
                color: rgba(255,255,255,0.78);
            }

            .secondary-settings-button:hover { background: alpha(white, 0.10); }
            .secondary-settings-button:active { background: alpha(white, 0.14); }

            /* ── Light-mode shortcut capture dialog ── */

            .shortcut-capture-dialog.editor-theme-light {
                background: #f6f7fb;
                border: 1px solid alpha(#111827, 0.08);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-title {
                color: #1d2129;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-subtitle {
                color: rgba(29, 33, 41, 0.72);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-hint {
                color: rgba(29, 33, 41, 0.45);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-listening-icon {
                color: #b05c38;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-keycap {
                background: alpha(#111827, 0.06);
                color: #17202a;
                border: none;
                box-shadow: none;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-plus {
                color: rgba(29, 33, 41, 0.35);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-cleared-label {
                color: rgba(29, 33, 41, 0.55);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-primary-btn {
                background: #b05c38;
                color: white;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-primary-btn:hover {
                background: #c06540;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-primary-btn:disabled {
                background: alpha(#111827, 0.06);
                color: alpha(#17202a, 0.35);
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-secondary-btn {
                background: alpha(#111827, 0.04);
                color: #17202a;
                border: none;
            }

            .shortcut-capture-dialog.editor-theme-light .shortcut-capture-secondary-btn:hover {
                background: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light .secondary-settings-button {
                border-color: alpha(#111827, 0.18);
                color: #17202a;
            }

            .editor-root.editor-theme-light .secondary-settings-button:hover {
                background: alpha(#111827, 0.06);
                color: #17202a;
            }

            .editor-root.editor-theme-light .secondary-settings-button:active {
                background: alpha(#111827, 0.10);
                color: #17202a;
            }

            /* FILENAME TAG PILLS */
            .filename-tag-pill {
                background-color: #fce4d6;
                color: #b05c38;
                border: none;
                border-radius: 4px;
                padding: 2px 8px;
                font-family: monospace;
                font-weight: bold;
                box-shadow: none;
            }
            .filename-tag-pill:hover {
                background-color: #f8d0b5;
            }

            .format-palette-box {
                background-color: alpha(@window_fg_color, 0.05); /* Adaptive gray */
                border: 1px solid alpha(@window_fg_color, 0.1);
                border-radius: 8px;
                padding: 20px;
            }

            .format-entry {
                background: @view_bg_color;
                color: @view_fg_color;
                border: 1px solid alpha(@window_fg_color, 0.2);
                border-radius: 6px;
                padding: 10px;
                font-size: 14px;
            }

            .modal-container {
                background-color: @window_bg_color;
                border-radius: 12px;
            }

            /* ABOUT TAB STYLES */
            .about-app-name {
                font-size: 24px;
                font-weight: 800;
                margin-bottom: 4px;
            }
            .about-version-label {
                font-size: 13px;
                opacity: 0.6;
                margin-bottom: 24px;
            }
            .about-link-button {
                background: transparent;
                border: none;
                color: #b05c38;
                font-size: 13px;
                padding: 4px 8px;
                border-radius: 4px;
                transition: all 0.15s ease;
            }
            .about-link-button:hover {
                background: alpha(#b05c38, 0.08);
                color: #b05c38;
                text-decoration: underline;
            }
            .editor-root.editor-theme-light .about-link-button {
                color: #9a4c2c;
            }
            .editor-root.editor-theme-light .about-link-button:hover {
                color: #9a4c2c;
            }

            /* Onboarding */
            .onboarding-back-button {
                padding: 4px 14px;
                min-height: 24px;
                font-size: 12px;
            }

            .onboarding-dot {
                min-width: 6px;
                min-height: 6px;
                border-radius: 999px;
                background-color: alpha(white, 0.18);
                padding: 0;
                margin: 0 2px;
            }

            .onboarding-dot-active {
                background-color: #b05c38;
                min-width: 18px;
            }

            .editor-root.editor-theme-light .onboarding-dot {
                background-color: alpha(#111827, 0.14);
            }

            .editor-root.editor-theme-light .onboarding-dot-active {
                background-color: #b05c38;
            }

            .cloud-avatar {
                background-color: #bb6d7a;
                color: white;
                border-radius: 50%;
                font-size: 24px;
                font-weight: bold;
            }

            .cloud-user-name {
                font-weight: bold;
                font-size: 16px;
            }

            .cloud-user-email {
                font-size: 13px;
                opacity: 0.6;
            }

            /* COHESIVE SETTINGS-LIKE DESIGN */
            .recent-captures-root {
                /* Intentionally empty to let .editor-root's background and borders shine through natively */
            }

            .recent-captures-toolbar-status {
                font-size: 12px;
                font-weight: 400;
                opacity: 0.5;
            }

            .recent-captures-statusbar {
                padding: 6px 0;
                border-top: 1px solid alpha(white, 0.06);
            }

            .recent-captures-shell {
                margin: 0;
            }

            .recent-captures-header {
                padding: 10px 0;
                margin-bottom: 24px;
            }

            .recent-captures-title {
                font-size: 26px;
                font-weight: 700;
            }

            .recent-captures-subtitle {
                font-size: 13px;
                opacity: 0.7;
            }

            .recent-captures-hero {
                padding: 0;
                border-radius: 12px;
                background: transparent;
                border: none;
            }

            .recent-captures-hero-image {
                border-radius: 8px;
                border: 1px solid alpha(white, 0.1);
                background: alpha(white, 0.04);
            }

            .recent-captures-card-image {
                border-radius: 8px;
                border: 1px solid alpha(white, 0.1);
                background: alpha(white, 0.04);
            }

            .recent-captures-list-row {
                padding: 12px;
                border-radius: 12px;
                transition: background 0.2s;
            }

            .recent-captures-list-row:hover {
                background: alpha(white, 0.03);
            }

            .recent-captures-list-row.recent-captures-card-alt {
                background: alpha(white, 0.015);
            }
            .recent-captures-list-row.recent-captures-card-alt:hover {
                background: alpha(white, 0.03);
            }

            .recent-captures-hero-meta {
                min-width: 320px;
                padding-left: 20px;
                margin-top: 8px;
            }

            .recent-captures-hero-title {
                font-size: 20px;
                font-weight: 700;
            }

            .recent-captures-hero-timestamp {
                font-size: 13px;
                font-weight: 500;
                color: #b05c38;
                margin-top: 4px;
            }

            .recent-captures-hero-supporting {
                font-size: 13px;
                line-height: 1.5;
                opacity: 0.8;
                margin-top: 10px;
                margin-bottom: 24px;
            }

            .recent-captures-hero-actions {
                margin-top: 24px;
            }

            .recent-captures-grid-title {
                font-size: 18px;
                font-weight: 700;
                margin-top: 32px;
                margin-bottom: 8px;
            }

            .recent-captures-grid {
                margin-top: 0;
            }

            button.recent-captures-card {
                padding: 12px;
                border-radius: 10px;
                border: none;
                background: transparent;
                box-shadow: none;
                transition: opacity 0.2s ease, transform 0.2s ease;
            }

            button.recent-captures-card:hover,
            button.recent-captures-card:focus {
                background: transparent;
                opacity: 0.7;
            }

            button.recent-captures-card-alt {
                margin-top: 0px; 
            }

            .recent-captures-card-title {
                font-size: 14px;
                font-weight: 700;
                margin-top: 10px;
            }

            .recent-captures-card-timestamp {
                font-size: 12px;
                font-weight: 500;
                color: #b05c38;
                margin-top: 4px;
            }

            .recent-captures-card-meta {
                font-size: 12px;
                opacity: 0.6;
                margin-top: 2px;
            }

            .recent-captures-empty-state {
                padding: 48px 20px;
                border-radius: 12px;
                background: alpha(white, 0.03);
                border: 1px solid alpha(white, 0.08);
            }

            .recent-captures-empty-title {
                font-size: 18px;
                font-weight: 700;
            }

            .recent-captures-empty-detail {
                font-size: 14px;
                opacity: 0.7;
                margin-top: 8px;
            }

            .recent-captures-primary-button,
            .recent-captures-secondary-button,
            .recent-captures-refresh-button {
                background: alpha(white, 0.08);
                border: 1px solid alpha(white, 0.1);
                border-radius: 6px;
                padding: 6px 14px;
                font-size: 12px;
                font-weight: 600;
                color: white;
                transition: all 0.2s;
            }

            .recent-captures-primary-button {
                background-color: #b05c38;
                border-color: #9a4c2c;
                color: white;
            }

            .recent-captures-primary-button:hover,
            .recent-captures-primary-button:focus {
                background-color: #c06540;
            }

            .recent-captures-secondary-button:hover,
            .recent-captures-secondary-button:focus,
            .recent-captures-refresh-button:hover,
            .recent-captures-refresh-button:focus {
                background-color: alpha(white, 0.12);
            }

            .recent-captures-icon-btn {
                background: transparent;
                border: none;
                border-radius: 6px;
                padding: 6px 8px;
                color: #b05c38;
                opacity: 0.8;
                transition: opacity 0.2s, background 0.2s;
            }

            .recent-captures-icon-btn:hover {
                background: alpha(white, 0.08);
                opacity: 1.0;
            }

            .recent-captures-picture-missing {
                background: alpha(white, 0.04);
            }

            .recent-captures-media-badge {
                color: white;
                background: alpha(black, 0.5);
                border: 1px solid alpha(white, 0.2);
                border-radius: 99px;
                padding: 12px;
            }

            button.recent-captures-wm-btn {
                min-width: 28px;
                min-height: 28px;
                padding: 4px;
                border-radius: 6px;
                background: transparent;
                background-image: none;
                color: alpha(white, 0.65);
                border: none;
                box-shadow: none;
                outline: none;
                transition: background 0.15s, color 0.15s;
            }
            button.recent-captures-wm-btn:hover,
            button.recent-captures-wm-btn:active,
            button.recent-captures-wm-btn:focus {
                background: alpha(white, 0.1);
                background-image: none;
                border: none;
                box-shadow: none;
                outline: none;
                color: white;
            }
            button.recent-captures-wm-close:hover {
                background: alpha(#e34a4a, 0.75);
                color: white;
            }

            .editor-root.editor-theme-light button.recent-captures-wm-btn {
                background: transparent;
                background-image: none;
                color: alpha(#111827, 0.58);
                border: none;
                box-shadow: none;
                outline: none;
            }

            .editor-root.editor-theme-light button.recent-captures-wm-btn:hover,
            .editor-root.editor-theme-light button.recent-captures-wm-btn:active,
            .editor-root.editor-theme-light button.recent-captures-wm-btn:focus {
                background: alpha(#111827, 0.08);
                background-image: none;
                color: #17202a;
                border: none;
                box-shadow: none;
                outline: none;
            }

            .editor-root.editor-theme-light button.recent-captures-wm-close:hover {
                background: alpha(#e34a4a, 0.82);
                color: white;
            }

            .editor-root .settings-window-controls button.recording-editor-traffic-btn {
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
            }

            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn {
                color: #1d2129;
                background: transparent;
                background-image: none;
                border: none;
                box-shadow: none;
            }

            .editor-root .settings-window-controls button.recording-editor-traffic-btn:hover,
            .editor-root .settings-window-controls button.recording-editor-traffic-btn:active,
            .editor-root .settings-window-controls button.recording-editor-traffic-btn:focus {
                background: alpha(white, 0.10);
                background-image: none;
                color: white;
                border: none;
                border-radius: 999px;
                box-shadow: none;
            }

            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:hover,
            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:active,
            .editor-root.editor-theme-light .settings-window-controls button.recording-editor-traffic-btn:focus {
                background: alpha(#111827, 0.10);
                background-image: none;
                color: #1d2129;
                border: none;
                border-radius: 999px;
                box-shadow: none;
            }

            .recent-captures-segmented-control {
                background: alpha(white, 0.05);
                border-radius: 8px;
                padding: 4px;
            }
            .recent-captures-segmented-btn {
                background: transparent;
                border: none;
                border-radius: 6px;
                padding: 6px 16px;
                color: alpha(white, 0.6);
                font-size: 13px;
                font-weight: 500;
                transition: background 0.2s, color 0.2s, box-shadow 0.2s;
            }
            .recent-captures-segmented-btn:hover {
                color: white;
            }
            .recent-captures-segmented-btn:checked {
                background: #b05c38;
                color: white;
                box-shadow: 0 1px 3px alpha(black, 0.2);
            }

            /* LIGHT THEME OVERRIDES (Match settings UI light theme) */
            .editor-root.editor-theme-light .recent-captures-hero-image,
            .editor-root.editor-theme-light .recent-captures-card-image {
                border-color: alpha(#111827, 0.12);
                background: alpha(#111827, 0.04);
            }

            .editor-root.editor-theme-light button.recent-captures-card:hover,
            .editor-root.editor-theme-light button.recent-captures-card:focus {
                background: transparent;
                opacity: 0.7;
            }

            .editor-root.editor-theme-light .recent-captures-primary-button {
                background-color: #b05c38;
                border-color: #9a4c2c;
                color: white;
            }

            .editor-root.editor-theme-light .recent-captures-primary-button:hover,
            .editor-root.editor-theme-light .recent-captures-primary-button:focus {
                background-color: #c06540;
            }

            .editor-root.editor-theme-light .recent-captures-secondary-button,
            .editor-root.editor-theme-light .recent-captures-refresh-button {
                background-color: transparent;
                color: #1d2129;
                border-color: alpha(#111827, 0.2);
            }

            .editor-root.editor-theme-light .recent-captures-secondary-button:hover,
            .editor-root.editor-theme-light .recent-captures-secondary-button:focus,
            .editor-root.editor-theme-light .recent-captures-refresh-button:hover,
            .editor-root.editor-theme-light .recent-captures-refresh-button:focus {
                background-color: alpha(#111827, 0.06);
            }
            .editor-root.editor-theme-light .recent-captures-empty-state {
                background: alpha(#111827, 0.03);
                border-color: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light .recent-captures-icon-btn {
                color: #9a4c2c;
            }

            .editor-root.editor-theme-light .recent-captures-icon-btn:hover {
                background: alpha(#111827, 0.06);
            }

            .editor-root.editor-theme-light .recent-captures-segmented-control {
                background: alpha(black, 0.05);
            }
            .editor-root.editor-theme-light .recent-captures-segmented-btn {
                color: alpha(black, 0.6);
            }
            .editor-root.editor-theme-light .recent-captures-segmented-btn:hover {
                color: black;
            }
            .editor-root.editor-theme-light .recent-captures-segmented-btn:checked {
                background: #b05c38;
                color: white;
                box-shadow: 0 1px 3px alpha(black, 0.1);
            }
            .noir-gallery-window {
                background-color: #141414;
                color: #F1F1F3;
            }
            .noir-header {
                background-color: #141414;
                border-bottom: 1px solid alpha(white, 0.10);
            }
            .noir-content {
                background: transparent;
            }
            .noir-gallery {
                margin: 0;
            }
            .noir-card {
                border-radius: 10px;
                transition: transform 0.3s ease, box-shadow 0.3s ease;
                box-shadow: 0 4px 12px alpha(black, 0.3);
                border: 1px solid alpha(white, 0.10);
            }
            .noir-card-button {
                padding: 0;
                margin: 0;
                border: none;
                background: transparent;
            }
            .noir-card-image {
                border-radius: 10px;
            }
            .noir-card:hover {
                box-shadow: 0 12px 30px alpha(black, 0.8);
                border-color: alpha(white, 0.15);
            }
            .noir-card-meta {
                background: linear-gradient(to top, rgba(0,0,0,0.95) 0%, rgba(0,0,0,0.4) 60%, rgba(0,0,0,0) 100%);
                padding: 40px 16px 16px 16px;
                border-radius: 0 0 12px 12px;
                opacity: 0;
                transition: opacity 0.3s ease;
            }
            .noir-card:hover .noir-card-meta {
                opacity: 1;
            }
            .noir-card-title {
                color: white;
                font-size: 15px;
                font-weight: 600;
            }
            .noir-card-action {
                background: alpha(white, 0.1);
                color: white;
                border: none;
                border-radius: 50%;
                padding: 6px;
                margin-left: 6px;
                transition: background 0.2s;
            }
            .noir-card-action:hover {
                background: alpha(white, 0.25);
            }
            .noir-card-tag {
                background: alpha(black, 0.5);
                color: white;
                border: 1px solid alpha(white, 0.1);
                padding: 4px 10px;
                border-radius: 8px;
                font-size: 11px;
                font-weight: bold;
                margin: 12px;
            }
            "#;

pub fn install_settings_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(SETTINGS_CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::SETTINGS_CSS;

    #[test]
    fn settings_css_avoids_unsupported_gtk_properties() {
        for property in ["max-width", "overflow", "backdrop-filter"] {
            assert!(
                !SETTINGS_CSS.contains(property),
                "settings CSS still contains unsupported GTK property: {property}"
            );
        }
    }
}

pub fn traffic_light_button(color_class: &str, tooltip: &str) -> Button {
    let icon_name = match color_class {
        "traffic-light-red" => "window-close-symbolic",
        "traffic-light-yellow" => "window-minimize-symbolic",
        "traffic-light-green" => "window-maximize-symbolic",
        _ => "window-close-symbolic",
    };

    let button = Button::builder()
        .icon_name(icon_name)
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .build();

    button.add_css_class("recent-captures-wm-btn");
    if color_class == "traffic-light-red" {
        button.add_css_class("recent-captures-wm-close");
    }

    button
}
