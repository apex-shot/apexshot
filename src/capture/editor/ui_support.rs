use super::state::EditorState;
use super::types::{AnnotationAction, DrawColor, Point};
use gtk4::gdk;
use gtk4::{
    glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea, Entry,
    EventControllerKey, Image, Label, Orientation, Window,
};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub fn parse_env_bool(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn read_gsettings(schema: &str, key: &str) -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    Some(
        raw.trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_ascii_lowercase(),
    )
}

pub fn read_gsettings_bool(schema: &str, key: &str) -> Option<bool> {
    match read_gsettings(schema, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn prefers_dark_glass_theme() -> bool {
    if let Some(settings) = gtk4::Settings::default() {
        if settings.property::<bool>("gtk-application-prefer-dark-theme") {
            return true;
        }

        let theme_name = settings
            .property::<Option<String>>("gtk-theme-name")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if theme_name.contains("dark") {
            return true;
        }
        if theme_name.contains("light") {
            return false;
        }
    }

    if let Some(color_scheme) = read_gsettings("org.gnome.desktop.interface", "color-scheme") {
        if color_scheme.contains("prefer-dark") {
            return true;
        }
        if color_scheme.contains("prefer-light") {
            return false;
        }
    }

    true
}

pub fn prefers_reduced_transparency() -> bool {
    if let Some(value) = parse_env_bool("CLEANSHITX_REDUCED_TRANSPARENCY") {
        return value;
    }

    if let Some(settings) = gtk4::Settings::default() {
        let theme_name = settings
            .property::<Option<String>>("gtk-theme-name")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if theme_name.contains("highcontrast") {
            return true;
        }

        if !settings.property::<bool>("gtk-enable-animations") {
            return true;
        }
    }

    if read_gsettings_bool("org.gnome.desktop.a11y.interface", "high-contrast").unwrap_or(false) {
        return true;
    }

    if let Some(animations_enabled) =
        read_gsettings_bool("org.gnome.desktop.interface", "enable-animations")
    {
        return !animations_enabled;
    }

    false
}

pub fn install_editor_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            window,
            window.editor-window,
            .editor-window {
                background-color: transparent;
                background: none;
                border: none;
                box-shadow: none;
            }

            .editor-root {
                border-radius: 10px;
                background-color: #141414;
                border: 1px solid rgba(255, 255, 255, 0.10);
                color: #F1F1F3;
                box-shadow: none;
            }

            .editor-toolbar {
                padding: 8px 12px;
                background-color: #141414;
                border-bottom: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 10px 10px 0 0;
            }

            .editor-toolbar-brand {
                font-size: 0;
                margin: 0;
                padding: 0;
                color: transparent;
            }

            .editor-toolbar-left,
            .editor-toolbar-center,
            .editor-toolbar-right,
            .editor-toolbar-right-tools {
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
                background: transparent;
                background-image: none;
                box-shadow: none;
            }

            button.traffic-light:hover,
            button.traffic-light:active,
            button.traffic-light:focus {
                background: transparent;
                box-shadow: none;
                outline: none;
            }

            .traffic-light-dot {
                min-width: 12px;
                min-height: 12px;
                border-radius: 999px;
                justify-content: center;
                align-items: center;
                transition: all 140ms ease;
                border: 1px solid rgba(0, 0, 0, 0.45);
            }

            .traffic-light-symbol {
                font-size: 8px;
                font-weight: 700;
                line-height: 1;
                color: rgba(0, 0, 0, 0.62);
                margin: 0;
                padding: 0;
                min-width: 12px;
                min-height: 12px;
                opacity: 0;
                transition: opacity 120ms ease;
            }

            button.traffic-light:hover .traffic-light-symbol,
            button.traffic-light:active .traffic-light-symbol {
                opacity: 1;
            }

            button.traffic-light:hover .traffic-light-dot {
                filter: brightness(1.08);
            }

            .traffic-light-dot.traffic-light-red {
                background: #ff5f57;
                border-color: #d8463f;
            }
            .traffic-light-dot.traffic-light-yellow {
                background: #febc2f;
                border-color: #d39a25;
            }
            .traffic-light-dot.traffic-light-green {
                background: #28c840;
                border-color: #20a736;
            }

            .traffic-light-dot.traffic-light-red .traffic-light-symbol { color: #5f1f1b; }
            .traffic-light-dot.traffic-light-yellow .traffic-light-symbol { color: #6d4f13; }
            .traffic-light-dot.traffic-light-green .traffic-light-symbol { color: #1a5f27; }

            .editor-tools-group {
                padding: 3px;
                border-radius: 6px;
                background-color: #000000;
                border: 1px solid rgba(255, 255, 255, 0.11);
                box-shadow: none;
            }

            .editor-primary-tools-group {
                padding-left: 6px;
                padding-right: 6px;
            }

            .editor-size-group.size-group-inactive {
                opacity: 0.42;
            }

            .editor-tools-divider {
                min-width: 1px;
                margin: 6px 8px;
                background-color: rgba(255, 255, 255, 0.11);
                border-radius: 2px;
            }

            button.editor-tool-button,
            button.standalone-tool {
                min-width: 30px;
                min-height: 30px;
                border-radius: 6px;
                padding: 0;
                margin: 0 1px;
                color: #9a9aa2;
                background-color: transparent;
                border: 1px solid transparent;
                outline: none;
                transition: all 120ms ease;
            }

            button.editor-tool-button:hover,
            button.standalone-tool:hover {
                color: #f2f2f4;
                background-color: #1a1a1d;
                border-color: rgba(255, 255, 255, 0.09);
            }

            button.editor-tool-button:active,
            button.standalone-tool:active {
                background-color: #151517;
                border-color: rgba(255, 255, 255, 0.15);
            }

            button.editor-tool-button.active-tool {
                background-color: #2a2a2a;
                color: #ffffff;
                border: 1px solid rgba(255, 255, 255, 0.15);
                box-shadow: none;
            }

            .editor-color-group {
                padding: 0;
                margin: 0 2px;
            }

            .editor-color-trigger-shell {
                min-height: 30px;
                border-radius: 999px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                background-image: linear-gradient(to bottom,
                    rgba(38, 38, 44, 0.95),
                    rgba(25, 25, 30, 0.95));
                padding: 2px 6px 2px 4px;
                transition: all 160ms ease;
                box-shadow:
                    0 1px 2px rgba(0, 0, 0, 0.35),
                    inset 0 1px 0 rgba(255, 255, 255, 0.06);
            }

            .editor-color-trigger-shell:hover {
                border-color: rgba(255, 255, 255, 0.18);
                background-image: linear-gradient(to bottom,
                    rgba(46, 46, 54, 0.98),
                    rgba(31, 31, 38, 0.98));
                box-shadow:
                    0 3px 8px rgba(0, 0, 0, 0.35),
                    inset 0 1px 0 rgba(255, 255, 255, 0.08);
            }

            .editor-color-trigger-shell:active {
                border-color: rgba(255, 255, 255, 0.12);
                background-image: linear-gradient(to bottom,
                    rgba(22, 22, 27, 0.98),
                    rgba(17, 17, 21, 0.98));
                box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.4);
            }

            button.editor-color-trigger-menu-button {
                min-width: 0;
                min-height: 0;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                background-image: none;
                box-shadow: none;
            }

            button.editor-color-trigger-menu-button > arrow {
                min-width: 0;
                min-height: 0;
                opacity: 0;
                color: transparent;
            }

            button.editor-color-trigger-menu-button:hover,
            button.editor-color-trigger-menu-button:active,
            button.editor-color-trigger-menu-button:focus {
                background: transparent;
                border: none;
                box-shadow: none;
                outline: none;
            }

            button.editor-color-trigger-menu-button image {
                min-width: 0;
                min-height: 0;
                opacity: 0;
            }

            .editor-color-trigger-dot {
                min-width: 20px;
                min-height: 20px;
                border-radius: 999px;
                border: 1px solid rgba(255, 255, 255, 0.24);
                box-shadow:
                    0 0 0 1px rgba(0, 0, 0, 0.5),
                    inset 0 1px 2px rgba(0, 0, 0, 0.22);
            }

            .editor-color-trigger-divider {
                min-width: 1px;
                min-height: 14px;
                margin: 0 5px;
                background: rgba(255, 255, 255, 0.11);
                border-radius: 1px;
            }

            .editor-color-trigger-arrow-box {
                min-width: 14px;
                min-height: 14px;
                padding: 0;
            }

            .editor-color-trigger-arrow {
                opacity: 0.72;
                transition: all 140ms ease;
            }

            .editor-color-trigger-shell:hover .editor-color-trigger-arrow {
                opacity: 1.0;
                transform: translateY(0.5px);
            }

            .editor-color-trigger-arrow-box image {
                filter: brightness(0) invert(1);
            }

            .editor-color-dot,
            .editor-color-placeholder-dot {
                min-width: 18px;
                min-height: 18px;
                border-radius: 999px;
                transition: transform 120ms ease;
            }

            .editor-color-dot {
                border: 1px solid rgba(255, 255, 255, 0.16);
                box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
            }

            popover.editor-color-popover,
            popover.editor-color-popover > contents {
                background: transparent;
                border: none;
                box-shadow: none;
                padding: 0;
            }

            .editor-color-popover-body {
                padding: 8px;
                border-radius: 14px;
                background-image: linear-gradient(to bottom,
                    rgba(28, 28, 34, 0.98),
                    rgba(18, 18, 23, 0.98));
                border: 1px solid rgba(255, 255, 255, 0.10);
                box-shadow:
                    0 14px 32px rgba(0, 0, 0, 0.55),
                    inset 0 1px 0 rgba(255, 255, 255, 0.04);
            }

            .editor-color-swatches-side {
                padding: 0;
            }

            .editor-color-dropdown-columns {
                margin-bottom: 4px;
            }

            .editor-color-dropdown-column {
                min-width: 32px;
                margin-top: 0;
                margin-bottom: 0;
            }

            button.editor-color-button {
                min-width: 26px;
                min-height: 26px;
                border-radius: 8px;
                padding: 3px;
                margin: 0;
                border: 1px solid transparent;
                background: rgba(255, 255, 255, 0.01);
                transition: all 130ms ease;
                box-shadow: none;
            }

            button.editor-color-button:hover {
                background: rgba(255, 255, 255, 0.07);
                border-color: rgba(255, 255, 255, 0.09);
            }

            button.editor-color-button:hover .editor-color-dot {
                transform: scale(1.03);
            }

            button.editor-color-button.active-color {
                background: rgba(255, 255, 255, 0.08);
                border-color: rgba(255, 255, 255, 0.16);
            }

            .editor-color-popover-body *:drop(active) {
                border-color: transparent;
                outline: none;
                box-shadow: none;
            }

            button.editor-color-button:drop(active),
            button.editor-custom-color-slot:drop(active),
            .editor-custom-color-slot-overlay:drop(active) {
                border-color: transparent;
                background: transparent;
                outline: none;
                box-shadow: none;
            }

            .editor-color-placeholder-dot {
                border: 1px dashed rgba(255, 255, 255, 0.24);
                background: rgba(255, 255, 255, 0.03);
            }

            .editor-custom-color-slot-overlay {
                min-width: 26px;
                min-height: 26px;
            }

            button.editor-custom-color-slot:hover .editor-color-placeholder-dot {
                border-color: rgba(255, 255, 255, 0.38);
                background: rgba(255, 255, 255, 0.08);
            }

            button.editor-custom-color-slot:drop(active) .editor-color-placeholder-dot {
                border-color: rgba(255, 255, 255, 0.5);
                background: rgba(255, 255, 255, 0.1);
                transform: scale(1.05);
            }

            button.editor-custom-color-slot.has-custom-color:hover .editor-color-dot {
                box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.34), 0 2px 5px rgba(0, 0, 0, 0.35);
            }

            button.editor-custom-color-slot.has-custom-color:drop(active) .editor-color-dot {
                box-shadow: 0 0 0 2px #16161a, 0 0 0 4px rgba(255, 255, 255, 0.5);
                transform: scale(1.05);
            }

            .editor-custom-color-slot-overlay:drop(active) button.editor-custom-color-slot {
                background: transparent;
                border-color: transparent;
            }

            button.editor-custom-color-remove-button {
                min-width: 9px;
                min-height: 9px;
                border-radius: 999px;
                padding: 0;
                border: 1px solid rgba(255, 255, 255, 0.24);
                background: #0f0f12;
                color: #ffffff;
                outline: none;
                box-shadow: 0 1px 3px rgba(0,0,0,0.35);
                transition: transform 150ms ease;
            }

            button.editor-custom-color-remove-button:hover,
            button.editor-custom-color-remove-button:active,
            button.editor-custom-color-remove-button:focus,
            button.editor-custom-color-remove-button:focus-visible {
                border: 1px solid rgba(255, 255, 255, 0.34);
                background: #1a1a20;
                color: #ffffff;
                outline: none;
                box-shadow: 0 2px 5px rgba(0,0,0,0.42);
                transform: scale(1.10);
            }

            button.editor-custom-color-remove-button image.editor-custom-color-remove-icon {
                color: #ffffff;
                opacity: 0.96;
            }

            .editor-color-dropdown-footer {
                padding: 4px;
                margin-top: 3px;
                border-radius: 10px;
                border: 1px solid rgba(255, 255, 255, 0.08);
                background: rgba(10, 10, 14, 0.6);
                box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
            }

            button.editor-universal-color-button {
                min-width: 30px;
                min-height: 30px;
                border-radius: 8px;
                border: 1px solid rgba(255, 255, 255, 0.1);
                background: rgba(255, 255, 255, 0.04);
                padding: 0;
                transition: all 150ms ease;
            }

            button.editor-universal-color-button:hover {
                background: rgba(255, 255, 255, 0.11);
                border-color: rgba(255, 255, 255, 0.2);
                box-shadow: 0 2px 5px rgba(0,0,0,0.25);
            }

            button.editor-universal-color-button:active {
                background: rgba(255, 255, 255, 0.05);
                border-color: rgba(255, 255, 255, 0.14);
            }

            .editor-universal-color-wheel {
                min-width: 22px;
                min-height: 22px;
                border-radius: 7px;
                border: 1px solid rgba(255, 255, 255, 0.22);
                box-shadow: inset 0 0 0 1px rgba(0,0,0,0.24);
                background-image: linear-gradient(135deg,
                    #ff3b30 0%,
                    #ff9500 18%,
                    #ffd60a 34%,
                    #30d158 50%,
                    #64d2ff 66%,
                    #0a84ff 82%,
                    #bf5af2 100%);
            }

            button.editor-universal-arrow-button {
                min-width: 30px;
                min-height: 30px;
                border-radius: 8px;
                border: 1px solid rgba(255, 255, 255, 0.1);
                background: rgba(255, 255, 255, 0.04);
                padding: 0;
                transition: all 150ms ease;
            }

            button.editor-universal-arrow-button:hover {
                background: rgba(255, 255, 255, 0.11);
                border-color: rgba(255, 255, 255, 0.2);
                box-shadow: 0 2px 5px rgba(0,0,0,0.25);
            }

            button.editor-universal-arrow-button:active {
                background: rgba(255, 255, 255, 0.05);
                border-color: rgba(255, 255, 255, 0.14);
            }

            .editor-picker-back-arrow {
                -gtk-icon-transform: rotate(180deg);
            }

            .editor-color-picker-panel {
                min-width: 252px;
                padding: 0 0 0 10px;
                margin-left: 8px;
                border-left: 1px solid rgba(255, 255, 255, 0.08);
            }

            .editor-gradient-area {
                min-width: 252px;
                min-height: 150px;
                border-radius: 11px;
                border: 1px solid rgba(255, 255, 255, 0.14);
                box-shadow:
                    inset 0 1px 4px rgba(0,0,0,0.32),
                    0 2px 4px rgba(0, 0, 0, 0.2);
            }

            .editor-hue-slider trough {
                min-height: 10px;
                border-radius: 999px;
                box-shadow: inset 0 1px 2px rgba(0,0,0,0.45);
                border: 1px solid rgba(255, 255, 255, 0.14);
                background-image: linear-gradient(to right,
                    #ff0000 0%, #ffff00 17%, #00ff00 33%,
                    #00ffff 50%, #0000ff 67%, #ff00ff 83%, #ff0000 100%);
            }

            .editor-hue-slider slider {
                min-width: 14px;
                min-height: 14px;
                border-radius: 999px;
                background: white;
                border: 1px solid rgba(0, 0, 0, 0.18);
                box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
                transition: transform 100ms ease;
            }

            .editor-hue-slider slider:hover,
            .editor-opacity-slider slider:hover {
                transform: scale(1.08);
            }

            .editor-hue-slider highlight {
                background: transparent;
            }

            .editor-opacity-slider trough {
                min-height: 10px;
                border-radius: 999px;
                box-shadow: inset 0 1px 2px rgba(0,0,0,0.45);
                border: 1px solid rgba(255, 255, 255, 0.14);
                background-image:
                    linear-gradient(45deg,
                        rgba(255, 255, 255, 0.14) 25%,
                        rgba(0, 0, 0, 0.0) 25%,
                        rgba(0, 0, 0, 0.0) 75%,
                        rgba(255, 255, 255, 0.14) 75%,
                        rgba(255, 255, 255, 0.14) 100%),
                    linear-gradient(45deg,
                        rgba(0, 0, 0, 0.11) 25%,
                        rgba(0, 0, 0, 0.0) 25%,
                        rgba(0, 0, 0, 0.0) 75%,
                        rgba(0, 0, 0, 0.11) 75%,
                        rgba(0, 0, 0, 0.11) 100%),
                    linear-gradient(to right,
                        rgba(55, 128, 91, 0.0) 0%,
                        rgba(55, 128, 91, 1.0) 100%);
                background-size: 8px 8px, 8px 8px, 100% 100%;
                background-position: 0 0, 4px 4px, 0 0;
            }

            .editor-opacity-slider slider {
                min-width: 14px;
                min-height: 14px;
                border-radius: 999px;
                background: white;
                border: 1px solid rgba(0, 0, 0, 0.18);
                box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
                transition: transform 100ms ease;
            }

            .editor-opacity-slider highlight {
                background: transparent;
            }

            .editor-color-preview {
                min-width: 24px;
                min-height: 24px;
                border-radius: 999px;
                background: #37805B;
                border: 1px solid rgba(255, 255, 255, 0.22);
                box-shadow:
                    inset 0 1px 3px rgba(0,0,0,0.28),
                    0 1px 2px rgba(0,0,0,0.26);
            }

            button.editor-eyedropper-button {
                min-width: 30px;
                min-height: 30px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                border-radius: 7px;
                background: rgba(255, 255, 255, 0.04);
                padding: 0;
                box-shadow: 0 1px 2px rgba(0,0,0,0.24);
                transition: all 150ms ease;
            }

            button.editor-eyedropper-button:hover {
                background: rgba(255, 255, 255, 0.11);
                border-color: rgba(255, 255, 255, 0.18);
                transform: translateY(-1px);
                box-shadow: 0 2px 5px rgba(0,0,0,0.3);
            }

            button.editor-eyedropper-button:active {
                background: rgba(255, 255, 255, 0.05);
                transform: translateY(0);
                box-shadow: none;
            }

            button.editor-eyedropper-button image {
                margin: 0;
            }

            window.editor-eyedropper-picker-window {
                background: transparent;
            }

            .editor-eyedropper-surface {
                background: #000000;
            }

            .editor-screen-eyedropper-ring {
                min-width: 132px;
                min-height: 132px;
                border-radius: 999px;
                border: none;
                box-shadow: none;
                background: transparent;
            }

            entry.editor-hex-entry,
            entry.editor-rgba-entry {
                min-height: 34px;
                border-radius: 7px;
                padding: 2px 8px;
                background: rgba(8, 8, 12, 0.55);
                border: 1px solid rgba(255, 255, 255, 0.10);
                box-shadow: inset 0 1px 2px rgba(0,0,0,0.28);
                color: #f7f8ff;
                font-size: 13px;
                font-family: 'DejaVu Sans Mono', 'Liberation Mono', Monospace;
                font-style: normal;
                font-weight: 600;
                transition: all 150ms ease;
            }

            entry.editor-hex-entry:focus,
            entry.editor-rgba-entry:focus {
                border-color: rgba(114, 167, 255, 0.72);
                background: rgba(12, 12, 18, 0.78);
                box-shadow:
                    0 0 0 2px rgba(82, 144, 255, 0.16),
                    inset 0 1px 2px rgba(0,0,0,0.32);
            }

            entry.editor-hex-entry text,
            entry.editor-rgba-entry text {
                color: #f7f8ff;
                text-align: center;
            }

            .editor-color-field-label {
                font-size: 10px;
                font-weight: 600;
                color: rgba(255, 255, 255, 0.52);
                margin-top: 1px;
                text-transform: uppercase;
                letter-spacing: 0.65px;
            }

            button.editor-add-to-colors-button {
                min-width: 220px;
                min-height: 36px;
                border-radius: 8px;
                background: #326ce8;
                color: #ffffff;
                font-weight: 700;
                font-size: 12px;
                border: 1px solid rgba(137, 178, 255, 0.55);
                padding: 0 16px;
                outline: none;
                transition: all 150ms ease;
                box-shadow:
                    0 2px 6px rgba(0, 0, 0, 0.28),
                    inset 0 1px 0 rgba(255, 255, 255, 0.18);
            }

            button.editor-add-to-colors-button:hover {
                background: #3a79ff;
                border-color: rgba(173, 203, 255, 0.66);
                box-shadow:
                    0 4px 10px rgba(0, 0, 0, 0.35),
                    inset 0 1px 0 rgba(255, 255, 255, 0.2);
                transform: translateY(-1px);
            }

            button.editor-add-to-colors-button:active {
                background: #2c5ec9;
                box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.35);
                transform: translateY(0);
            }

            .editor-color-dot.editor-color-black,
            .editor-color-trigger-dot.editor-color-black { background: #121212; }
            .editor-color-dot.editor-color-blue,
            .editor-color-trigger-dot.editor-color-blue { background: #0a84ff; }
            .editor-color-dot.editor-color-dark-green,
            .editor-color-trigger-dot.editor-color-dark-green { background: #005933; }
            .editor-color-dot.editor-color-red,
            .editor-color-trigger-dot.editor-color-red { background: #eb2424; }
            .editor-color-dot.editor-color-orange,
            .editor-color-trigger-dot.editor-color-orange { background: #ff9900; }
            .editor-color-dot.editor-color-yellow,
            .editor-color-trigger-dot.editor-color-yellow { background: #ffd601; }
            .editor-color-dot.editor-color-green,
            .editor-color-trigger-dot.editor-color-green { background: #29ba5c; }
            .editor-color-dot.editor-color-cyan,
            .editor-color-trigger-dot.editor-color-cyan { background: #00cfc7; }
            .editor-color-dot.editor-color-blue-bright,
            .editor-color-trigger-dot.editor-color-blue-bright { background: #338efb; }
            .editor-color-dot.editor-color-purple,
            .editor-color-trigger-dot.editor-color-purple { background: #9e5cfb; }
            .editor-color-dot.editor-color-pink,
            .editor-color-trigger-dot.editor-color-pink { background: #ff1478; }
            .editor-color-dot.editor-color-white,
            .editor-color-trigger-dot.editor-color-white {
                background: #f2f2f2;
                border: 1px solid rgba(255, 255, 255, 0.16);
            }

            button.editor-tool-button image,
            button.standalone-tool image,
            button.editor-footer-icon-button image,
            button.editor-universal-arrow-button image,
            button.editor-eyedropper-button image,
            .editor-color-trigger-arrow-box image {
                filter: brightness(0) invert(0.96);
            }

            button.editor-done-button {
                min-width: 68px;
                min-height: 30px;
                border-radius: 6px;
                padding: 0 16px;
                background-color: #f5f5f7;
                border: 1px solid #f5f5f7;
                color: #080808;
                font-size: 13px;
                font-weight: 700;
                outline: none;
                transition: all 120ms ease;
                box-shadow: none;
            }

            button.editor-done-button:hover {
                background-color: #ffffff;
                border-color: #ffffff;
                color: #050505;
            }

            button.editor-done-button:active {
                background-color: #dfdfe2;
                border-color: #dfdfe2;
            }

            .editor-footer {
                padding: 6px 12px;
                background-color: #141414;
                border-top: 1px solid rgba(255, 255, 255, 0.08);
                border-radius: 0 0 10px 10px;
            }

            button.editor-footer-icon-button {
                min-width: 30px;
                min-height: 30px;
                border-radius: 6px;
                border: 1px solid transparent;
                background: transparent;
                color: #9b9ba2;
                transition: all 120ms ease;
            }

            button.editor-footer-icon-button:hover {
                background: #1a1a1d;
                color: #ffffff;
                border-color: rgba(255, 255, 255, 0.11);
            }

            button.editor-footer-drag-button {
                min-width: 112px;
                min-height: 30px;
                border-radius: 6px;
                border: 1px solid rgba(255, 255, 255, 0.12);
                background: #121214;
                color: #d2d2d7;
                font-size: 13px;
                font-weight: 600;
                transition: all 120ms ease;
                box-shadow: none;
            }

            button.editor-footer-drag-button:hover {
                background: #1a1a1d;
                color: #ffffff;
                border-color: rgba(255, 255, 255, 0.2);
            }

            .editor-root.editor-theme-dark,
            .editor-root.editor-theme-light {
                background-color: #141414;
                color: #f1f1f3;
                border-color: rgba(255, 255, 255, 0.10);
                box-shadow: none;
            }

            .editor-root.editor-theme-dark .editor-toolbar,
            .editor-root.editor-theme-light .editor-toolbar {
                background-color: #141414;
                border-bottom-color: rgba(255, 255, 255, 0.08);
            }

            .editor-root.editor-theme-dark .editor-footer,
            .editor-root.editor-theme-light .editor-footer {
                background-color: #141414;
                border-top-color: rgba(255, 255, 255, 0.08);
            }

            .editor-root.editor-theme-dark button.editor-tool-button image,
            .editor-root.editor-theme-dark button.standalone-tool image,
            .editor-root.editor-theme-dark button.editor-footer-icon-button image,
            .editor-root.editor-theme-light button.editor-tool-button image,
            .editor-root.editor-theme-light button.standalone-tool image,
            .editor-root.editor-theme-light button.editor-footer-icon-button image {
                filter: brightness(0) invert(0.96);
            }

            .editor-root.editor-reduced-transparency.editor-theme-dark,
            .editor-root.editor-reduced-transparency.editor-theme-dark .editor-toolbar,
            .editor-root.editor-reduced-transparency.editor-theme-dark .editor-footer,
            .editor-root.editor-reduced-transparency.editor-theme-light,
            .editor-root.editor-reduced-transparency.editor-theme-light .editor-toolbar,
            .editor-root.editor-reduced-transparency.editor-theme-light .editor-footer {
                background-image: none;
            }

            .editor-canvas-frame {
                border-radius: 0;
                border: none;
                background-color: transparent;
                padding: 0;
            }

            .editor-canvas {
                border-radius: 0;
                background-color: transparent;
                border: none;
            }

            window.editor-text-modal-window,
            .editor-text-modal-window {
                background-color: transparent;
                background: none;
                border: none;
                box-shadow: none;
            }

            .editor-text-modal {
                min-width: 360px;
                padding: 14px;
                border-radius: 8px;
                background-color: #101013;
                border: 1px solid rgba(255, 255, 255, 0.12);
            }

            .editor-text-modal-title {
                color: #f1f1f3;
                font-size: 13px;
                font-weight: 700;
                margin-bottom: 2px;
            }

            entry.editor-text-modal-entry {
                min-height: 36px;
                border-radius: 6px;
                padding: 0 10px;
                color: #f1f1f3;
                background-color: #17171b;
                border: 1px solid rgba(255, 255, 255, 0.14);
            }

            entry.editor-text-modal-entry:focus {
                border-color: rgba(10, 132, 255, 0.68);
                box-shadow: none;
            }

            .editor-text-modal-actions {
                margin-top: 4px;
            }

            button.editor-text-modal-button {
                min-height: 32px;
                border-radius: 6px;
                padding: 0 12px;
                border: 1px solid rgba(255, 255, 255, 0.12);
                background: #17171b;
                color: #ececf0;
            }

            button.editor-text-modal-button:hover {
                background: #1d1d21;
            }

            button.editor-text-modal-confirm {
                background: #f5f5f7;
                border-color: #f5f5f7;
                color: #080808;
                font-weight: 700;
            }

            button.editor-text-modal-confirm:hover {
                background: #ffffff;
                border-color: #ffffff;
            }

            button.editor-text-modal-confirm:disabled {
                opacity: 0.45;
            }
            
            ",
        );

        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

pub fn icon_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let image = Image::from_icon_name(icon_name);
    image.set_pixel_size(14);

    let button = Button::new();
    button.set_child(Some(&image));
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("editor-tool-button");
    button
}

pub fn color_swatch_button(color_class: &str, tooltip: &str) -> Button {
    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.set_size_request(18, 18);
    dot.set_halign(gtk4::Align::Center);
    dot.set_valign(gtk4::Align::Center);
    dot.add_css_class("editor-color-dot");
    dot.add_css_class(color_class);

    let button = Button::new();
    button.set_child(Some(&dot));
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("editor-color-button");
    button
}

pub fn footer_icon_button(icon_name: &str, tooltip: &str) -> (Button, Image) {
    let image = Image::from_icon_name(icon_name);
    image.set_pixel_size(14);

    let button = Button::new();
    button.set_child(Some(&image));
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("editor-footer-icon-button");

    (button, image)
}

pub fn traffic_light_button(color_class: &str, tooltip: &str) -> Button {
    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.set_size_request(12, 12);
    dot.set_halign(gtk4::Align::Center);
    dot.set_valign(gtk4::Align::Center);
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
    symbol_label.set_halign(gtk4::Align::Center);
    symbol_label.set_valign(gtk4::Align::Center);
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

pub fn recommended_window_size(image_width: i32, image_height: i32) -> (i32, i32) {
    let (screen_width, screen_height) = if let Some(display) = gdk::Display::default() {
        let monitors = display.monitors();
        if monitors.n_items() > 0 {
            if let Some(obj) = monitors.item(0) {
                if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                    let geometry = monitor.geometry();
                    (geometry.width(), geometry.height())
                } else {
                    (1920, 1080)
                }
            } else {
                (1920, 1080)
            }
        } else {
            (1920, 1080)
        }
    } else {
        (1920, 1080)
    };

    let max_width = (screen_width as f64) * 0.90;
    let max_height = (screen_height as f64) * 0.85;

    let ui_height = 110.0;
    let ui_width = 72.0;
    let min_editor_width = 980.0_f64.min(max_width);
    let min_editor_height = 560.0_f64.min(max_height);

    let avail_width = (max_width - ui_width).max(1.0);
    let avail_height = (max_height - ui_height).max(1.0);

    let mut w = image_width as f64;
    let mut h = image_height as f64;

    if w > avail_width || h > avail_height {
        let scale_x = avail_width / w;
        let scale_y = avail_height / h;
        let scale = scale_x.min(scale_y);

        w *= scale;
        h *= scale;
    }

    (
        (w + ui_width).round().max(min_editor_width.round()) as i32,
        (h + ui_height).round().max(min_editor_height.round()) as i32,
    )
}

pub fn set_active_tool_button(buttons: &[Button], active_index: usize) {
    for (index, button) in buttons.iter().enumerate() {
        if index == active_index {
            button.add_css_class("active-tool");
        } else {
            button.remove_css_class("active-tool");
        }
    }
}

pub fn set_active_color_button(buttons: &[Button], active_index: usize) {
    for (index, button) in buttons.iter().enumerate() {
        if index == active_index {
            button.add_css_class("active-color");
        } else {
            button.remove_css_class("active-color");
        }
    }
}

pub fn set_crop_apply_button_state(button: &Button, crop_mode: bool, has_selection: bool) {
    if let Some(slot) = button
        .parent()
        .and_then(|parent| parent.downcast::<GtkBox>().ok())
    {
        if slot.has_css_class("crop-apply-slot") {
            slot.set_visible(crop_mode);
        }
    }
    button.set_visible(crop_mode);
    button.set_sensitive(crop_mode && has_selection);
}

fn show_text_modal<F>(
    parent: &ApplicationWindow,
    title: &str,
    confirm_label: &str,
    placeholder: Option<&str>,
    initial_text: Option<&str>,
    allow_empty: bool,
    on_submit: F,
) where
    F: Fn(String) + 'static,
{
    let modal = Window::builder()
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .resizable(false)
        .build();
    modal.add_css_class("editor-text-modal-window");

    let body = GtkBox::new(Orientation::Vertical, 10);
    body.add_css_class("editor-text-modal");

    let title_label = Label::new(Some(title));
    title_label.set_halign(gtk4::Align::Start);
    title_label.add_css_class("editor-text-modal-title");

    let entry = Entry::new();
    entry.add_css_class("editor-text-modal-entry");
    if let Some(placeholder) = placeholder {
        entry.set_placeholder_text(Some(placeholder));
    }
    if let Some(initial_text) = initial_text {
        entry.set_text(initial_text);
        entry.select_region(0, -1);
    }

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(gtk4::Align::End);
    actions.add_css_class("editor-text-modal-actions");

    let cancel_btn = Button::with_label("Cancel");
    cancel_btn.set_has_frame(false);
    cancel_btn.add_css_class("editor-text-modal-button");

    let confirm_btn = Button::with_label(confirm_label);
    confirm_btn.set_has_frame(false);
    confirm_btn.add_css_class("editor-text-modal-button");
    confirm_btn.add_css_class("editor-text-modal-confirm");

    if !allow_empty {
        confirm_btn.set_sensitive(
            initial_text
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false),
        );
        let confirm_btn_state = confirm_btn.clone();
        entry.connect_changed(move |input| {
            confirm_btn_state.set_sensitive(!input.text().trim().is_empty());
        });
    }

    actions.append(&cancel_btn);
    actions.append(&confirm_btn);

    body.append(&title_label);
    body.append(&entry);
    body.append(&actions);
    modal.set_child(Some(&body));

    let on_submit: Rc<dyn Fn(String)> = Rc::new(on_submit);

    let modal_cancel = modal.downgrade();
    cancel_btn.connect_clicked(move |_| {
        if let Some(dialog) = modal_cancel.upgrade() {
            dialog.close();
        }
    });

    let modal_confirm = modal.downgrade();
    let entry_confirm = entry.clone();
    let on_submit_confirm = on_submit.clone();
    confirm_btn.connect_clicked(move |_| {
        let raw_text = entry_confirm.text().to_string();
        let trimmed_text = raw_text.trim().to_string();
        if !allow_empty && trimmed_text.is_empty() {
            return;
        }

        let text = if allow_empty { raw_text } else { trimmed_text };
        on_submit_confirm(text);

        if let Some(dialog) = modal_confirm.upgrade() {
            dialog.close();
        }
    });

    let modal_activate = modal.downgrade();
    let on_submit_activate = on_submit.clone();
    entry.connect_activate(move |input| {
        let raw_text = input.text().to_string();
        let trimmed_text = raw_text.trim().to_string();
        if !allow_empty && trimmed_text.is_empty() {
            return;
        }

        let text = if allow_empty { raw_text } else { trimmed_text };
        on_submit_activate(text);

        if let Some(dialog) = modal_activate.upgrade() {
            dialog.close();
        }
    });

    let modal_escape = modal.downgrade();
    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            if let Some(dialog) = modal_escape.upgrade() {
                dialog.close();
            }
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });
    modal.add_controller(key_controller);

    modal.present();
    entry.grab_focus();
}

pub fn show_text_dialog(
    parent: &ApplicationWindow,
    state: Arc<Mutex<EditorState>>,
    position: Point,
    color: DrawColor,
    font_size: f64,
    drawing_area: glib::WeakRef<DrawingArea>,
) {
    show_text_modal(
        parent,
        "Add text",
        "Add",
        Some("Type text"),
        None,
        false,
        move |text| {
            state.lock().unwrap().push_action(AnnotationAction::Text {
                position,
                text,
                color,
                font_size,
            });
            if let Some(area) = drawing_area.upgrade() {
                area.queue_draw();
            }
        },
    );
}

pub fn show_text_edit_dialog(
    parent: &ApplicationWindow,
    state: Arc<Mutex<EditorState>>,
    action_index: usize,
    current_text: &str,
    drawing_area: glib::WeakRef<DrawingArea>,
) {
    show_text_modal(
        parent,
        "Edit text",
        "Apply",
        None,
        Some(current_text),
        true,
        move |next_text| {
            let changed = state
                .lock()
                .unwrap()
                .update_text_action(action_index, next_text);
            if changed {
                if let Some(area) = drawing_area.upgrade() {
                    area.queue_draw();
                }
            }
        },
    );
}
