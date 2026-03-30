use gtk4::gdk;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, CssProvider, Label, Orientation};

pub fn install_settings_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            r#"
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

            .editor-toolbar-left,
            .editor-toolbar-right {
                min-height: 32px;
            }

            .editor-traffic-lights {
                margin-left: 0;
                margin-right: 10px;
            }

            button.traffic-light {
                min-width: 14px;
                min-height: 14px;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background-color: transparent;
                background-image: none;
            }

            button.traffic-light:hover,
            button.traffic-light:active,
            button.traffic-light:focus {
                background-color: transparent;
                background-image: none;
                border: none;
                outline-width: 0;
            }

            .traffic-light-dot {
                min-width: 12px;
                min-height: 12px;
                border-radius: 999px;
                border: 1px solid alpha(black, 0.45);
            }

            .traffic-light-symbol {
                font-size: 8px;
                font-weight: 700;
                color: alpha(black, 0.62);
                margin: 0;
                padding: 0;
                min-width: 12px;
                min-height: 12px;
                opacity: 0;
            }

            button.traffic-light:hover .traffic-light-symbol,
            button.traffic-light:active .traffic-light-symbol {
                opacity: 1;
            }

            button.traffic-light:hover .traffic-light-dot {
                opacity: 0.94;
            }

            .traffic-light-dot.traffic-light-red {
                background-color: #ff5f57;
                border-color: #d8463f;
            }

            .traffic-light-dot.traffic-light-yellow {
                background-color: #febc2f;
                border-color: #d39a25;
            }

            .traffic-light-dot.traffic-light-green {
                background-color: #28c840;
                border-color: #20a736;
            }

            .traffic-light-dot.traffic-light-red .traffic-light-symbol {
                color: #5f1f1b;
            }

            .traffic-light-dot.traffic-light-yellow .traffic-light-symbol {
                color: #6d4f13;
            }

            .traffic-light-dot.traffic-light-green .traffic-light-symbol {
                color: #1a5f27;
            }

            .editor-root.editor-theme-light {
                background-color: #f6f7fb;
                color: #1d2129;
                border-color: alpha(#111827, 0.10);
            }

            .editor-root.editor-theme-light .editor-toolbar {
                background-color: #f6f7fb;
            }

            .editor-root.editor-reduced-transparency {
                background-color: #111318;
            }

            .editor-root.editor-reduced-transparency.editor-theme-light {
                background-color: #ffffff;
            }

            .settings-nav-strip {
                padding: 0 32px 12px 32px;
                border-bottom: 1px solid alpha(white, 0.08);
            }

            .settings-nav-item {
                min-width: 57px;
                min-height: 38px;
                padding: 8px 6px 6px 6px;
                border-radius: 10px;
            }

            .settings-nav-item-hover,
            .settings-nav-item-selected {
                background-color: alpha(#eeeeee, 0.16);
            }

            .settings-nav-icon {
                opacity: 0.92;
            }

            .settings-nav-label {
                font-size: 12px;
                margin-top: 6px;
            }

            .settings-nav-icon-hover,
            .settings-nav-label-hover,
            .settings-nav-icon-selected,
            .settings-nav-label-selected {
                color: #4aa3ff;
            }

            .editor-root.editor-theme-light .settings-nav-strip {
                border-bottom-color: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light .settings-nav-item-hover,
            .editor-root.editor-theme-light .settings-nav-item-selected {
                background-color: alpha(#111827, 0.06);
            }

            .editor-root.editor-theme-light .settings-nav-icon,
            .editor-root.editor-theme-light .settings-nav-label {
                color: #1d2129;
            }

            .editor-root.editor-theme-light .settings-nav-icon-hover,
            .editor-root.editor-theme-light .settings-nav-label-hover,
            .editor-root.editor-theme-light .settings-nav-icon-selected,
            .editor-root.editor-theme-light .settings-nav-label-selected {
                color: #1976d2;
            }

            .settings-save-status {
                font-size: 12px;
                font-weight: 600;
                opacity: 0.92;
                margin-right: 4px;
            }

            .settings-save-status-success {
                color: #59d88d;
            }

            .settings-save-status-error {
                color: #ff8f84;
            }

            .editor-root.editor-theme-light .settings-save-status-success {
                color: #1f8a4c;
            }

            .editor-root.editor-theme-light .settings-save-status-error {
                color: #c93d2b;
            }

            .settings-group-title {
                font-size: 15px;
                font-weight: 700;
            }

            .settings-sub-option {
                font-size: 12px;
                opacity: 0.84;
            }

            .settings-table-header {
                font-size: 13px;
                font-weight: 700;
            }

            .settings-table-frame {
                border-radius: 14px;
                border: 1px solid alpha(white, 0.10);
            }

            .editor-root.editor-theme-light .settings-table-frame {
                border-color: alpha(#111827, 0.10);
            }

            .settings-table-row {
                padding: 10px 16px;
            }

            .settings-table-row-muted {
                background-color: alpha(white, 0.04);
            }

            .editor-root.editor-theme-light .settings-table-row-muted {
                background-color: alpha(#111827, 0.04);
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
                min-width: 220px;
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
                color: alpha(white, 0.9);
                background-color: alpha(white, 0.05);
            }

            .recording-tab-button.active {
                background-color: alpha(white, 0.1);
                color: white;
                box-shadow: 0 2px 4px alpha(black, 0.2);
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
                border-color: #4aa3ff;
                background-color: alpha(#4aa3ff, 0.08);
                box-shadow: 0 4px 12px alpha(black, 0.3);
            }

            .mode-icon-check {
                opacity: 0;
                width: 0;
                height: 0;
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

            .secondary-settings-button {
                background: none;
                border: 1px solid alpha(white, 0.15);
                border-radius: 6px;
                padding: 4px 12px;
                font-size: 12px;
                transition: all 0.2s;
            }

            .secondary-settings-button:hover { background: #e5e5e5; }
            .secondary-settings-button:active { background: #d5d5d5; }

            /* FILENAME TAG PILLS */
            .filename-tag-pill {
                background-color: #d1e7ff;
                color: #007aff;
                border: none;
                border-radius: 4px;
                padding: 2px 8px;
                font-family: monospace;
                font-weight: bold;
                box-shadow: none;
            }
            .filename-tag-pill:hover {
                background-color: #b9daff;
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
                color: @link_color;
                font-size: 13px;
                box-shadow: none;
            }
            .about-link-button:hover {
                background: transparent;
                text-decoration: underline;
                color: shade(@link_color, 0.8);
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
            "#,
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn traffic_light_button(color_class: &str, tooltip: &str) -> Button {
    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.set_size_request(12, 12);
    dot.set_halign(Align::Center);
    dot.set_valign(Align::Center);
    dot.add_css_class("traffic-light-dot");
    dot.add_css_class(color_class);

    let symbol = match color_class {
        "traffic-light-red" => "x",
        "traffic-light-yellow" => "-",
        "traffic-light-green" => "+",
        _ => "",
    };
    let symbol_label = Label::new(Some(symbol));
    symbol_label.add_css_class("traffic-light-symbol");
    symbol_label.set_halign(Align::Center);
    symbol_label.set_valign(Align::Center);
    symbol_label.set_xalign(0.5);
    symbol_label.set_yalign(0.5);
    dot.append(&symbol_label);

    let button = Button::new();
    button.set_size_request(14, 14);
    button.set_child(Some(&dot));
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("traffic-light");
    button.add_css_class("flat");
    button
}
