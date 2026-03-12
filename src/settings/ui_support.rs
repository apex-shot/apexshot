use gtk4::gdk;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, CssProvider, Label, Orientation};

pub fn install_settings_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            r#"
            window,
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
