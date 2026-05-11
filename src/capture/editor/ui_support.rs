use super::types::ArrowStyle;
use gtk4::gdk;
use gtk4::{
    prelude::*, Box as GtkBox, Button, CssProvider, DrawingArea, Image, Orientation, Widget,
};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorToolIcon {
    Named(String),
    ArrowStyle(ArrowStyle),
}

pub fn arrow_style_toolbar_icon(style: ArrowStyle) -> EditorToolIcon {
    EditorToolIcon::ArrowStyle(style)
}

pub fn toolbar_icon_size(icon: &EditorToolIcon) -> i32 {
    match icon {
        EditorToolIcon::Named(_) => 14,
        _ => 14,
    }
}

fn custom_toolbar_icon_inset(icon: &EditorToolIcon) -> f64 {
    match icon {
        EditorToolIcon::ArrowStyle(_) => 2.1,
        EditorToolIcon::Named(_) => 0.0,
    }
}

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
    if let Some(value) = parse_env_bool("APEXSHOT_REDUCED_TRANSPARENCY") {
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
                padding: 2px 10px 2px 12px;
                background-color: #141414;
                border-bottom: 1px solid alpha(white, 0.06);
                border-radius: 10px 10px 0 0;
            }

            .editor-toolbar button.recording-editor-traffic-btn {
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
                outline: none;
            }

            .editor-toolbar button.recording-editor-traffic-btn image {
                -gtk-icon-size: 14px;
            }

            .editor-toolbar button.recording-editor-traffic-btn:hover,
            .editor-toolbar button.recording-editor-traffic-btn:active,
            .editor-toolbar button.recording-editor-traffic-btn:focus {
                background-color: rgba(255, 255, 255, 0.10);
                background-image: none;
                color: #ffffff;
                border-radius: 999px;
                border: none;
                box-shadow: none;
            }

            .editor-toolbar button.recording-editor-traffic-btn:hover image,
            .editor-toolbar button.recording-editor-traffic-btn:active image,
            .editor-toolbar button.recording-editor-traffic-btn:focus image {
                color: #ffffff;
            }

            .editor-toolbar-wm-controls {
                padding-left: 6px;
                border-left: 1px solid rgba(255, 255, 255, 0.06);
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
                min-height: 26px;
            }

            .editor-traffic-lights {
                margin-left: 0;
                margin-right: 10px;
            }

            .recent-captures-wm-btn {
                min-width: 28px;
                min-height: 28px;
                padding: 4px;
                border-radius: 6px;
                background: transparent;
                color: alpha(white, 0.65);
                transition: background 0.15s, color 0.15s;
            }
            .recent-captures-wm-btn:hover {
                background: alpha(white, 0.1);
                color: white;
            }
            .recent-captures-wm-close:hover {
                background: alpha(#e34a4a, 0.75);
                color: white;
            }

            /* Flat group container used by history group on the right side. */
            .editor-tools-group {
                padding: 0;
                border-radius: 0;
                background-color: transparent;
                border: none;
                box-shadow: none;
            }

            /* Tool subgroups are invisible spacers — just structural for spacing. */
            .editor-tools-subgroup {
                padding: 0;
                background: transparent;
                border: none;
                box-shadow: none;
            }

            .editor-primary-tools-row {
                padding: 0;
            }

            .editor-crop-mode-group {
                padding: 0;
                background: transparent;
                border: none;
            }

            .editor-crop-size-group {
                padding-left: 8px;
                padding-right: 8px;
            }

            .editor-crop-type-shell {
                padding: 0 10px 0 12px;
                box-shadow: none;
            }

            .editor-crop-type-shell:hover {
                box-shadow: none;
            }

            .editor-crop-type-shell:active {
                box-shadow: none;
            }

            .editor-crop-type-group {
                padding: 0 1px;
            }

            button.editor-crop-type-button {
                min-width: 68px;
                min-height: 30px;
                padding: 0;
                margin: 0;
                border: 1px solid rgba(255, 255, 255, 0.11);
                border-radius: 6px;
                background-color: #000000;
                background-image: none;
                box-shadow: none;
            }

            button.editor-crop-type-button > arrow {
                min-width: 20px;
                min-height: 20px;
                opacity: 0;
                color: transparent;
            }

            button.editor-crop-type-button:hover,
            button.editor-crop-type-button:active,
            button.editor-crop-type-button:focus {
                background-color: #000000;
                border: 1px solid rgba(255, 255, 255, 0.11);
                box-shadow: none;
                outline: none;
            }

            button.editor-crop-type-button image {
                min-width: 20px;
                min-height: 20px;
                opacity: 0;
            }

            .editor-crop-type-label {
                color: #f3f3f5;
                font-size: 13px;
            }

            entry.editor-crop-size-entry {
                min-height: 30px;
                border-radius: 8px;
                border: none;
                background-color: #000000;
                background-image: none;
                color: #f3f3f5;
                padding: 0 8px;
                box-shadow: none;
            }

            entry.editor-crop-size-entry text {
                color: #f7f8ff;
                font-size: 13px;
            }

            .editor-crop-size-separator {
                color: rgba(243, 243, 245, 0.74);
                font-size: 13px;
                margin-left: 2px;
                margin-right: 2px;
            }

            .editor-crop-type-arrow-box {
                min-width: 14px;
                min-height: 14px;
                padding: 0;
            }

            .editor-crop-type-arrow {
                opacity: 0.76;
                transition: all 140ms ease;
            }

            .editor-crop-type-shell:hover .editor-crop-type-arrow {
                opacity: 1.0;
                transform: translateY(0.5px);
            }

            .editor-crop-type-arrow-box image {
                filter: brightness(0) invert(1);
            }

            popover.editor-crop-type-popover,
            popover.editor-crop-type-popover > contents {
                background: transparent;
                border: none;
                box-shadow: none;
                padding: 0;
            }

            .editor-crop-type-popover-body {
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

            button.editor-crop-type-option {
                min-width: 136px;
                min-height: 30px;
                padding: 0 12px;
                border-radius: 8px;
                border: 1px solid transparent;
                background: rgba(255, 255, 255, 0.01);
                color: #f3f3f5;
                box-shadow: none;
                transition: all 130ms ease;
            }

            button.editor-crop-type-option:hover {
                background: rgba(255, 255, 255, 0.07);
                border-color: rgba(255, 255, 255, 0.09);
            }

            button.editor-crop-type-option:active {
                background: rgba(255, 255, 255, 0.04);
                border-color: rgba(255, 255, 255, 0.14);
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

            /* Flat tool buttons — settings-nav-item style: no borders, subtle alpha bg states. */
            button.editor-tool-button,
            button.standalone-tool {
                min-width: 28px;
                min-height: 28px;
                border-radius: 5px;
                padding: 0;
                margin: 0;
                color: rgba(255, 255, 255, 0.78);
                background-color: transparent;
                background-image: none;
                border: none;
                outline: none;
                box-shadow: none;
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-tool-button:hover,
            button.standalone-tool:hover {
                color: #ffffff;
                background-color: alpha(white, 0.06);
                border: none;
            }

            button.editor-tool-button:active,
            button.standalone-tool:active {
                background-color: alpha(white, 0.10);
                border: none;
            }

            /* Active tool: flat alpha(white, 0.14) bg - matches settings-nav-item-selected pattern. */
            button.editor-tool-button.active-tool {
                background-color: alpha(white, 0.14);
                color: #ffffff;
                border: none;
                box-shadow: none;
            }

            button.editor-tool-button.active-tool:hover {
                background-color: alpha(white, 0.18);
            }

            .editor-color-group {
                padding: 0;
                margin: 0 2px;
            }

            .editor-toolbar-color-status {
                min-height: 34px;
                padding: 0 6px;
                border: none;
                background: transparent;
            }

            .editor-toolbar-color-status-swatch {
                min-width: 14px;
                min-height: 14px;
                border-radius: 999px;
                border: 1px solid alpha(white, 0.18);
                background: #121212;
                box-shadow: none;
            }

            .editor-toolbar-color-status-text {
                padding: 0;
            }

            .editor-toolbar-color-status-label {
                color: rgba(245, 245, 247, 0.94);
                font-size: 12px;
                font-weight: 700;
                letter-spacing: 0.1px;
            }

            .editor-toolbar-color-status-hint {
                color: rgba(245, 245, 247, 0.42);
                font-size: 10px;
                font-weight: 500;
                margin-top: -1px;
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
                min-width: 20px;
                min-height: 20px;
                padding: 0;
                margin: 0;
                border: none;
                border-radius: 0;
                background: transparent;
                background-image: none;
                box-shadow: none;
            }

            button.editor-color-trigger-menu-button > arrow {
                min-width: 20px;
                min-height: 20px;
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
                min-width: 20px;
                min-height: 20px;
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
            }

            .editor-color-dot {
                border: 1px solid alpha(white, 0.16);
                box-shadow: none;
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
                border-radius: 6px;
                padding: 3px;
                margin: 0;
                border: none;
                background: transparent;
                transition: background-color 120ms ease;
                box-shadow: none;
            }

            button.editor-color-button:hover {
                background: alpha(white, 0.06);
                border: none;
            }

            button.editor-color-button.active-color {
                background: alpha(white, 0.14);
                border: none;
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
                border: 1px dashed alpha(white, 0.22);
                background: alpha(white, 0.03);
            }

            .editor-custom-color-slot-overlay {
                min-width: 26px;
                min-height: 26px;
            }

            button.editor-custom-color-slot:hover .editor-color-placeholder-dot {
                border-color: alpha(white, 0.36);
                background: alpha(white, 0.06);
            }

            button.editor-custom-color-slot:drop(active) .editor-color-placeholder-dot {
                border-color: alpha(white, 0.46);
                background: alpha(white, 0.10);
            }

            .editor-custom-color-slot-overlay:drop(active) button.editor-custom-color-slot {
                background: transparent;
                border-color: transparent;
            }

            button.editor-custom-color-remove-button {
                min-width: 12px;
                min-height: 12px;
                border-radius: 999px;
                padding: 0;
                border: 1px solid alpha(white, 0.20);
                background: #1a1a1a;
                color: rgba(255, 255, 255, 0.86);
                outline: none;
                box-shadow: none;
                transition: background-color 120ms ease, border-color 120ms ease;
            }

            button.editor-custom-color-remove-button:hover,
            button.editor-custom-color-remove-button:active,
            button.editor-custom-color-remove-button:focus,
            button.editor-custom-color-remove-button:focus-visible {
                border: 1px solid alpha(white, 0.36);
                background: #2a2a2a;
                color: #ffffff;
                outline: none;
                box-shadow: none;
            }

            button.editor-custom-color-remove-button image.editor-custom-color-remove-icon {
                color: #ffffff;
                opacity: 0.96;
                -gtk-icon-size: 10px;
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
                border-radius: 8px;
                border: 1px solid alpha(white, 0.08);
                box-shadow: none;
            }

            /* --- Sliders (mirrors the video editor's minimal style) --- */
            .editor-root scale {
                min-height: 20px;
            }

            .editor-root scale trough {
                min-height: 4px;
                border-radius: 999px;
                background: alpha(white, 0.08);
                border: none;
            }

            .editor-root scale highlight {
                min-height: 4px;
                border-radius: 999px;
                background: #b05c38;
            }

            .editor-root scale:disabled trough {
                background: alpha(white, 0.04);
            }

            .editor-root scale:disabled highlight {
                background: alpha(#b05c38, 0.42);
            }

            .editor-root.editor-theme-light scale trough {
                background: alpha(#111827, 0.08);
            }

            .editor-root.editor-theme-light scale highlight {
                background: #b05c38;
            }

            .editor-root.editor-theme-light scale:disabled trough {
                background: alpha(#111827, 0.04);
            }

            .editor-root.editor-theme-light scale:disabled highlight {
                background: alpha(#b05c38, 0.42);
            }

            .editor-hue-slider trough {
                min-height: 8px;
                border-radius: 999px;
                box-shadow: none;
                border: 1px solid alpha(white, 0.08);
                background-image: linear-gradient(to right,
                    #ff0000 0%, #ffff00 17%, #00ff00 33%,
                    #00ffff 50%, #0000ff 67%, #ff00ff 83%, #ff0000 100%);
            }

            .editor-hue-slider slider {
                min-width: 14px;
                min-height: 14px;
                border-radius: 999px;
                background: white;
                border: 1px solid alpha(black, 0.40);
                box-shadow: none;
            }

            .editor-hue-slider highlight {
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
                min-height: 30px;
                border-radius: 6px;
                padding: 2px 8px;
                background: alpha(white, 0.05);
                border: 1px solid alpha(white, 0.06);
                box-shadow: none;
                color: rgba(255, 255, 255, 0.92);
                font-size: 12px;
                font-family: 'DejaVu Sans Mono', 'Liberation Mono', Monospace;
                font-style: normal;
                font-weight: 600;
                transition: background-color 120ms ease, border-color 120ms ease;
            }

            entry.editor-hex-entry:focus,
            entry.editor-rgba-entry:focus {
                border-color: alpha(white, 0.20);
                background: alpha(white, 0.08);
                box-shadow: none;
            }

            entry.editor-hex-entry text,
            entry.editor-rgba-entry text {
                color: rgba(255, 255, 255, 0.92);
            }

            .editor-color-field-label {
                font-size: 10px;
                font-weight: 600;
                color: rgba(255, 255, 255, 0.52);
                margin-top: 1px;
                text-transform: uppercase;
                letter-spacing: 0.55px;
            }

            button.editor-add-to-colors-button {
                min-width: 220px;
                min-height: 30px;
                border-radius: 6px;
                background: #B05C38;
                color: #ffffff;
                font-weight: 600;
                font-size: 12px;
                border: none;
                padding: 0 14px;
                outline: none;
                transition: background-color 120ms ease;
                box-shadow: none;
            }

            button.editor-add-to-colors-button:hover {
                background: #C66B4A;
                border: none;
                box-shadow: none;
            }

            button.editor-add-to-colors-button:active {
                background: #8A4A2D;
                box-shadow: none;
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

            entry.editor-text-entry {
                background: transparent;
                border: none;
                box-shadow: none;
                padding: 0;
                margin: 0;
            }

            entry.editor-text-entry text {
                background: transparent;
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
                min-width: 72px;
                min-height: 26px;
                border-radius: 6px;
                padding: 0 14px;
                background: #b05c38;
                border: none;
                color: white;
                font-size: 12px;
                font-weight: 600;
                outline: none;
                transition: all 120ms ease;
                box-shadow: none;
            }

            button.editor-done-button label {
                color: white;
                font-size: 12px;
                font-weight: 600;
            }

            button.editor-done-button:hover {
                background: #c06a44;
            }

            button.editor-done-button:hover label {
                color: white;
            }

            button.editor-done-button:active {
                background: #8a4a2d;
            }

            /* --- Footer (settings-UI design language: flat, alpha-white states) --- */
            .editor-footer {
                padding: 6px 10px;
                min-height: 38px;
                background-color: #141414;
                border-top: 1px solid alpha(white, 0.06);
                border-radius: 0 0 10px 10px;
            }

            button.editor-footer-icon-button {
                min-width: 28px;
                min-height: 28px;
                border-radius: 5px;
                border: none;
                background: transparent;
                color: rgba(255, 255, 255, 0.78);
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-footer-icon-button:hover {
                background: alpha(white, 0.06);
                color: #ffffff;
                border: none;
            }

            button.editor-footer-icon-button:active {
                background: alpha(white, 0.10);
                border: none;
            }

            button.editor-footer-zoom-button {
                min-height: 28px;
                padding: 0 10px;
                border-radius: 5px;
                border: none;
                background: transparent;
                color: rgba(255, 255, 255, 0.86);
                box-shadow: none;
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-footer-zoom-button:hover {
                background: alpha(white, 0.06);
                color: #ffffff;
                border: none;
            }

            .editor-footer-zoom-label {
                color: inherit;
                font-size: 12px;
                font-weight: 600;
            }

            /* --- Zoom popup (flat surface, no glass) --- */
            .editor-footer-zoom-popup {
                padding: 0;
                background: #1a1a1a;
                border: 1px solid alpha(white, 0.06);
                border-radius: 10px;
                min-width: 240px;
                box-shadow: none;
            }

            .editor-footer-zoom-header {
                padding: 10px 10px 8px 10px;
                border-bottom: 1px solid alpha(white, 0.05);
            }

            button.editor-footer-zoom-header-btn {
                min-width: 24px;
                min-height: 24px;
                padding: 0;
                border-radius: 5px;
                background: alpha(white, 0.06);
                border: none;
                color: rgba(255, 255, 255, 0.86);
                font-weight: 600;
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-footer-zoom-header-btn:hover {
                background: alpha(white, 0.12);
                color: #ffffff;
            }

            button.editor-footer-zoom-header-btn.orange-btn {
                background: #b05c38;
                color: #ffffff;
            }

            button.editor-footer-zoom-header-btn.orange-btn:hover {
                background: #c06540;
                color: #ffffff;
            }

            .editor-footer-zoom-header-label {
                font-weight: 600;
                font-size: 13px;
                color: rgba(255, 255, 255, 0.92);
            }

            .editor-footer-zoom-list {
                padding: 4px;
            }

            button.editor-footer-zoom-action-btn {
                padding: 0;
                margin: 0;
                border-radius: 5px;
                border: none;
                background: transparent;
                transition: background-color 120ms ease;
            }

            button.editor-footer-zoom-action-btn:hover {
                background: alpha(white, 0.06);
            }

            .editor-footer-zoom-row {
                padding: 6px 10px;
                min-height: 30px;
                color: rgba(255, 255, 255, 0.86);
                font-size: 12px;
            }

            .editor-footer-zoom-shortcut-box {
                margin-left: 12px;
            }

            .editor-footer-zoom-shortcut-part {
                font-size: 10px;
                color: rgba(255, 255, 255, 0.62);
                font-weight: 600;
                background: alpha(white, 0.05);
                border: 1px solid alpha(white, 0.06);
                border-radius: 4px;
                padding: 1px 5px;
                min-width: 18px;
            }

            .editor-footer-zoom-separator {
                min-height: 1px;
                background: alpha(white, 0.05);
                margin: 4px 10px;
            }

            .editor-footer-zoom-mouse-hints {
                padding: 12px 10px 14px 10px;
            }

            .editor-footer-zoom-mouse-hint-text {
                font-size: 10px;
                color: rgba(255, 255, 255, 0.50);
                line-height: 1.35;
            }

            .editor-footer-zoom-mouse-drawing {
                min-width: 60px;
                min-height: 60px;
                margin: 0 8px;
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
                border-bottom-color: alpha(white, 0.06);
            }

            .editor-root.editor-theme-dark .editor-footer,
            .editor-root.editor-theme-light .editor-footer {
                background-color: #141414;
                border-top-color: rgba(255, 255, 255, 0.06);
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

            .editor-canvas-workspace {
                background-color: transparent;
            }

            .editor-right-inspector {
                min-width: 210px;
                background: #141414;
                border-left: 1px solid alpha(white, 0.06);
                padding: 0;
            }

            .editor-inspector-title {
                color: rgba(255, 255, 255, 0.92);
                font-size: 13px;
                font-weight: 600;
                letter-spacing: -0.1px;
            }

            .editor-inspector-tabs {
                margin-top: 16px;
                margin-bottom: 12px;
                margin-left: 16px;
                margin-right: 16px;
            }

            button.editor-inspector-tab-button {
                min-height: 20px;
                padding: 0;
                border-radius: 0;
                border: none;
                background: transparent;
                color: rgba(241, 241, 243, 0.82);
                box-shadow: none;
            }

            button.editor-inspector-tab-button:hover {
                background: transparent;
                color: #ffffff;
            }

            button.editor-inspector-tab-button.active-inspector-tab {
                background: transparent;
                border: none;
                color: #b05c38;
            }

            .editor-inspector-placeholder-shell {
                min-width: 210px;
                padding: 16px;
            }

            .editor-inspector-placeholder {
                color: rgba(255, 255, 255, 0.62);
                font-size: 11px;
                line-height: 1.4;
            }

            .editor-inspector-section {
                margin-bottom: 12px;
            }

            .editor-inspector-section-body {
                background: alpha(white, 0.04);
                border: 1px solid alpha(white, 0.06);
                border-radius: 6px;
            }

            .editor-inspector-option-list {
                background: transparent;
            }

            .editor-inspector-toggle-row {
                padding: 10px 12px;
            }

            .editor-inspector-toggle-row checkbutton {
                color: rgba(241, 241, 243, 0.9);
            }

            button.editor-crop-inspector-option {
                border-radius: 8px;
                color: rgba(241, 241, 243, 0.9);
            }

            button.editor-crop-inspector-option:hover {
                background: alpha(white, 0.06);
                color: #ffffff;
            }

            button.editor-crop-inspector-option.editor-crop-inspector-option-active {
                background: alpha(white, 0.10);
                border: none;
            }

            .editor-crop-inspector-check {
                color: #b05c38;
                font-size: 12px;
                font-weight: 600;
            }

            .editor-crop-dimensions-row {
                padding: 12px 0;
            }

            .editor-dimension-box {
                background: alpha(white, 0.04);
                border: 1px solid alpha(white, 0.06);
                border-radius: 6px;
                padding: 6px 10px;
                min-width: 72px;
                transition: background-color 120ms ease, border-color 120ms ease;
            }

            .editor-crop-dimensions-value {
                color: rgba(255, 255, 255, 0.94);
                font-size: 13px;
                font-weight: 600;
                font-family: 'DejaVu Sans Mono', 'Liberation Mono', Monospace;
            }

            .editor-crop-dimensions-separator {
                color: rgba(255, 255, 255, 0.42);
                font-size: 13px;
                font-weight: 500;
                margin: 0 4px;
            }

            .editor-dimension-label {
                color: rgba(255, 255, 255, 0.46);
                font-size: 9px;
                font-weight: 600;
                letter-spacing: 0.5px;
                text-transform: uppercase;
                margin-top: 2px;
            }

            button.editor-crop-action-button {
                min-height: 30px;
                padding: 0 12px;
                border-radius: 6px;
                font-size: 12px;
                font-weight: 600;
                box-shadow: none;
                transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
            }

            button.editor-crop-action-button-secondary {
                border: 1px solid alpha(white, 0.08);
                background: alpha(white, 0.04);
                color: rgba(255, 255, 255, 0.86);
            }

            button.editor-crop-action-button-secondary:hover {
                background: alpha(white, 0.08);
                border-color: alpha(white, 0.12);
                color: #ffffff;
            }

            button.editor-crop-action-button-primary {
                border: none;
                background: #b05c38;
                color: #ffffff;
            }

            button.editor-crop-action-button-primary:hover {
                background: #c06540;
                color: #ffffff;
            }

            button.editor-crop-action-button-primary:active {
                background: #8a4a2d;
            }

            button.editor-crop-action-button:disabled {
                opacity: 0.45;
            }

            /* Unified inspector option rows (settings-nav pattern). */
            button.editor-arrow-inspector-option,
            button.editor-text-inspector-option,
            button.editor-obfuscate-inspector-option,
            button.editor-number-style-option,
            button.editor-number-size-option {
                border-radius: 5px;
                color: rgba(255, 255, 255, 0.82);
                border: none;
                background: transparent;
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-arrow-inspector-option:hover,
            button.editor-text-inspector-option:hover,
            button.editor-obfuscate-inspector-option:hover,
            button.editor-number-style-option:hover,
            button.editor-number-size-option:hover {
                background: alpha(white, 0.06);
                color: #ffffff;
            }

            button.editor-arrow-inspector-option.editor-arrow-inspector-option-active,
            button.editor-text-inspector-option.editor-text-inspector-option-active,
            button.editor-obfuscate-inspector-option.editor-obfuscate-inspector-option-active,
            button.editor-number-style-option.editor-number-style-option-active,
            button.editor-number-size-option.editor-number-size-option-active {
                background: alpha(white, 0.10);
                border: none;
                color: #ffffff;
            }

            .editor-arrow-inspector-check,
            .editor-text-inspector-check,
            .editor-obfuscate-inspector-check,
            .editor-number-style-check,
            .editor-number-size-check {
                color: #b05c38;
                font-size: 12px;
                font-weight: 600;
            }

            .editor-number-start-label {
                color: rgba(255, 255, 255, 0.82);
                font-size: 11px;
                font-weight: 500;
            }

            .editor-number-start-entry {
                min-height: 30px;
                min-width: 48px;
                padding: 0 10px;
                border-radius: 6px;
                border: 1px solid alpha(white, 0.08);
                background: alpha(white, 0.04);
                color: rgba(255, 255, 255, 0.94);
                font-size: 12px;
                font-weight: 600;
                box-shadow: none;
                transition: background-color 120ms ease, border-color 120ms ease;
            }

            .editor-number-start-entry:focus {
                border-color: alpha(white, 0.20);
                background: alpha(white, 0.08);
            }

            button.editor-number-start-stepper {
                min-width: 28px;
                min-height: 30px;
                padding: 0;
                border-radius: 6px;
                border: 1px solid alpha(white, 0.08);
                background: alpha(white, 0.04);
                color: rgba(255, 255, 255, 0.86);
                font-size: 13px;
                font-weight: 600;
                box-shadow: none;
                transition: background-color 120ms ease, color 120ms ease;
            }

            button.editor-number-start-stepper:hover {
                background: alpha(white, 0.08);
                color: #ffffff;
            }

            button.editor-number-start-stepper:active {
                background: alpha(white, 0.12);
            }

            .editor-background-sidebar {
                min-width: 210px;
                padding: 16px;
                background: transparent;
                border-right: none;
            }

            .editor-background-sidebar-title {
                color: rgba(255, 255, 255, 0.92);
                font-size: 13px;
                font-weight: 600;
                letter-spacing: -0.1px;
            }

            .editor-background-sidebar-body {
                color: rgba(255, 255, 255, 0.62);
                font-size: 11px;
                line-height: 1.4;
            }

            .editor-background-sidebar-options {
                margin-top: 4px;
            }

            .editor-colors-panel {
                min-width: 210px;
                padding: 16px;
            }

            .editor-colors-panel-helper {
                color: rgba(255, 255, 255, 0.62);
                font-size: 11px;
                line-height: 1.4;
                margin-bottom: 2px;
            }

            .editor-colors-panel-section {
                margin-top: 2px;
            }

            .editor-colors-panel-current-row {
                min-height: 32px;
            }

            .editor-colors-panel-current-preview {
                min-width: 28px;
                min-height: 28px;
                border-radius: 8px;
                border: 1px solid rgba(255, 255, 255, 0.12);
                background: rgba(255, 255, 255, 0.08);
            }

            .editor-colors-panel-current-value {
                color: rgba(245, 245, 247, 0.92);
                font-size: 12px;
                font-weight: 600;
            }

            .editor-colors-panel-action-button {
                min-height: 30px;
                padding: 0 12px;
                border-radius: 6px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                background: rgba(255, 255, 255, 0.03);
                color: rgba(245, 245, 247, 0.9);
                box-shadow: none;
            }

            .editor-colors-panel-action-button:hover {
                background: alpha(white, 0.06);
                color: #ffffff;
            }

            .editor-background-gradients-section {
                margin-top: 10px;
            }

            .editor-background-section-title {
                color: rgba(255, 255, 255, 0.46);
                font-size: 10px;
                font-weight: 600;
                letter-spacing: 0.6px;
                text-transform: uppercase;
                margin-top: 4px;
            }

            button.editor-background-section-action-button {
                padding: 0;
                margin: 0;
                background: transparent;
                border: none;
                box-shadow: none;
                color: #b05c38;
                font-size: 10px;
                font-weight: 600;
                letter-spacing: 0.4px;
                text-transform: uppercase;
                transition: color 120ms ease;
            }

            button.editor-background-section-action-button:hover {
                color: #c06540;
                background: transparent;
            }

            button.editor-background-section-action-button:active {
                color: #8a4a2d;
                background: transparent;
            }

            .editor-background-gradients-grid {
                margin-top: 4px;
            }

            .editor-background-wallpaper-section {
                margin-top: 10px;
            }

            .editor-background-wallpaper-grid {
                margin-top: 4px;
            }

            .editor-background-wallpaper-row {
            }

            .editor-background-blurred-section {
                margin-top: 10px;
            }

            .editor-background-blurred-row {
            }

            .editor-background-plain-color-section {
                margin-top: 10px;
            }

            .editor-background-plain-color-grid {
                margin-top: 4px;
            }

            .editor-background-plain-color-row {
                min-height: 18px;
            }

            .editor-background-plain-color-cell {
                min-height: 18px;
            }

            .editor-background-plain-color-end-spacer {
                min-width: 0;
            }

            .editor-background-preview-spacer {
                min-height: 46px;
            }

            .editor-background-divider-row {
                margin-top: 12px;
                margin-bottom: 2px;
            }

            .editor-background-divider {
                min-height: 1px;
                margin-top: 0;
                margin-bottom: 0;
                background: rgba(255, 255, 255, 0.10);
            }

            .editor-background-padding-section {
                margin-top: 4px;
            }

            .editor-background-padding-slider-row {
                margin-top: 2px;
            }

            .editor-background-padding-slider {
                margin-top: 0;
            }

            .editor-background-padding-slider,
            .editor-background-compact-slider {
                min-width: 0;
                margin: 0;
            }

            .editor-toolbar-size-slider {
                margin: 0 4px;
            }

            .editor-background-compact-controls {
                margin-top: 8px;
            }

            .editor-background-compact-controls-row {
                margin-top: 2px;
            }

            .editor-background-compact-slider-section {
                margin-top: 0;
            }

            .editor-background-compact-title-spacer {
                min-height: 18px;
            }

            .editor-background-compact-slider-row {
                margin-top: 2px;
            }

            .editor-background-compact-slider {
                min-width: 32px;
            }

            .editor-background-compact-control-spacer {
                min-width: 0;
                min-height: 30px;
            }

            .editor-background-alignment-grid {
                margin-top: 2px;
            }

            .editor-background-alignment-row {
                margin-top: 0;
            }

            button.editor-background-alignment-button {
                min-height: 24px;
                min-width: 34px;
                border-radius: 5px;
                border: none;
                padding: 0;
                background: transparent;
                box-shadow: none;
                transition: background-color 120ms ease, box-shadow 120ms ease;
            }

            .editor-background-alignment-icon {
                min-width: 34px;
                min-height: 24px;
                border: none;
                border-radius: 5px;
            }

            .editor-background-alignment-icon-frame {
                min-width: 10px;
                min-height: 6px;
                background: rgba(255, 255, 255, 0.78);
                border-radius: 1px;
                margin: 3px;
                border: none;
            }

            button.editor-background-alignment-button:hover {
                background: alpha(white, 0.06);
            }

            button.editor-background-alignment-button:active,
            button.editor-background-alignment-button:focus-visible {
                background: alpha(white, 0.10);
            }

            .editor-background-checkbox-row {
                min-height: 30px;
                margin-top: 0;
            }

            checkbutton.editor-background-checkbox {
                color: rgba(255, 255, 255, 0.82);
                min-width: 20px;
                padding: 0;
                font-size: 11px;
            }

            checkbutton.editor-background-checkbox check {
                border-radius: 4px;
                background: alpha(white, 0.04);
                border: 1px solid alpha(white, 0.18);
                min-width: 14px;
                min-height: 14px;
            }

            checkbutton.editor-background-checkbox check:checked {
                background: #b05c38;
                border-color: #b05c38;
            }

            checkbutton.editor-background-checkbox label {
                color: rgba(255, 255, 255, 0.82);
                min-width: 20px;
            }

            .editor-background-ratio-dropdown-row {
                margin-top: 2px;
            }

            dropdown.editor-background-ratio-dropdown {
                min-height: 32px;
                min-width: 20px;
                padding: 0;
            }

            dropdown.editor-background-ratio-dropdown > button {
                min-height: 28px;
                min-width: 20px;
                border-radius: 6px;
                padding: 0 6px;
                background: rgba(255, 255, 255, 0.03);
                border: 1px solid rgba(255, 255, 255, 0.10);
                color: rgba(241, 241, 243, 0.88);
                box-shadow: none;
            }

            dropdown.editor-background-ratio-dropdown > button > box {
                border-spacing: 0;
            }

            dropdown.editor-background-ratio-dropdown > button:hover {
                background: alpha(white, 0.06);
                border-color: alpha(white, 0.12);
            }

            dropdown.editor-background-ratio-dropdown > button:active,
            dropdown.editor-background-ratio-dropdown > button:focus-visible {
                background: alpha(white, 0.10);
                border-color: alpha(white, 0.18);
            }

            /* Hide the tick/checkmark in dropdown popover */
            dropdown.editor-background-ratio-dropdown popover > contents listview row image {
                opacity: 0;
            }

            button.editor-background-option-button {
                min-height: 32px;
                border-radius: 6px;
                padding: 0 12px;
                background: alpha(white, 0.04);
                border: 1px solid alpha(white, 0.06);
                color: rgba(255, 255, 255, 0.82);
                box-shadow: none;
                transition: background-color 120ms ease, color 120ms ease, box-shadow 120ms ease;
            }

            button.editor-background-option-button:hover {
                background: alpha(white, 0.08);
                border-color: alpha(white, 0.10);
                color: #ffffff;
            }

            button.editor-background-option-button:active {
                background: alpha(white, 0.12);
                border-color: alpha(white, 0.14);
            }

            button.editor-background-option-button.active-background-option {
                background: alpha(white, 0.10);
                border: 1px solid transparent;
                color: #ffffff;
                box-shadow: inset 0 0 0 1px #b05c38;
            }

            button.editor-background-alignment-button.active-alignment-option {
                background: alpha(white, 0.10);
                border: none;
                border-radius: 5px;
                box-shadow: inset 0 0 0 1px #b05c38;
            }

            button.editor-background-gradient-button {
                padding: 0;
                background-color: alpha(white, 0.04);
                border: 1px solid alpha(white, 0.06);
                box-shadow: none;
                transition: box-shadow 120ms ease, border-color 120ms ease;
            }

            button.editor-background-gradient-button.active-background-option {
                border: 1px solid transparent;
                background-color: alpha(white, 0.06);
                box-shadow: inset 0 0 0 2px #b05c38;
            }

            .text-entry-overlay {
                background: transparent;
                border: none;
                outline: none;
            }

            button.editor-background-gradient-button.editor-background-preview-size-regular {
                min-width: 56px;
                min-height: 56px;
                border-radius: 12px;
            }

            button.editor-background-gradient-button.editor-background-preview-size-medium {
                min-width: 48px;
                min-height: 48px;
                border-radius: 11px;
            }

            button.editor-background-gradient-button.editor-background-preview-size-compact {
                min-width: 44px;
                min-height: 44px;
                border-radius: 10px;
            }

            button.editor-background-gradient-button:hover {
                border-color: alpha(white, 0.16);
                box-shadow: none;
            }

            button.editor-background-gradient-button:active {
                border-color: alpha(white, 0.22);
                box-shadow: none;
            }

            button.editor-background-add-button {
                padding: 0;
                background-color: alpha(white, 0.02);
                border: 1px dashed alpha(white, 0.22);
                color: rgba(255, 255, 255, 0.78);
                box-shadow: none;
                transition: background-color 120ms ease, border-color 120ms ease, color 120ms ease;
            }

            button.editor-background-add-button.editor-background-preview-size-regular {
                min-width: 56px;
                min-height: 56px;
                border-radius: 12px;
            }

            button.editor-background-add-button.editor-background-preview-size-medium {
                min-width: 48px;
                min-height: 48px;
                border-radius: 11px;
            }

            button.editor-background-add-button.editor-background-preview-size-compact {
                min-width: 44px;
                min-height: 44px;
                border-radius: 10px;
            }

            button.editor-background-add-button:hover {
                background-color: alpha(white, 0.06);
                border-color: alpha(white, 0.32);
                color: #ffffff;
            }

            button.editor-background-add-button:active {
                background-color: alpha(white, 0.04);
                border-color: alpha(white, 0.38);
            }

            .editor-background-add-label {
                font-weight: 500;
            }

            .editor-background-add-label.editor-background-preview-size-regular {
                font-size: 22px;
            }

            .editor-background-add-label.editor-background-preview-size-medium {
                font-size: 19px;
            }

            .editor-background-add-label.editor-background-preview-size-compact {
                font-size: 17px;
            }

            button.editor-background-blurred-button {
                background-image: linear-gradient(135deg, alpha(white, 0.16) 0%, alpha(white, 0.06) 100%);
                border: 1px solid alpha(white, 0.10);
            }

            button.editor-background-blurred-button.active-background-option {
                border: 1px solid transparent;
                background-color: alpha(white, 0.10);
                box-shadow: inset 0 0 0 2px #b05c38;
            }

            /* Blur intensity visual indicators */
            button.editor-background-blurred-button.blur-light {
                background: linear-gradient(135deg, rgba(220, 220, 220, 0.5) 0%, rgba(200, 200, 200, 0.3) 100%);
            }

            button.editor-background-blurred-button.blur-medium {
                background: linear-gradient(135deg, rgba(160, 160, 160, 0.6) 0%, rgba(130, 130, 130, 0.5) 100%);
            }

            button.editor-background-blurred-button.blur-heavy {
                background: linear-gradient(135deg, rgba(90, 90, 90, 0.7) 0%, rgba(60, 60, 60, 0.6) 100%);
            }

            .editor-blur-intensity-label {
                font-size: 14px;
                font-weight: bold;
                color: rgba(255, 255, 255, 0.9);
                text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
            }

            button.editor-background-plain-color-button {
                min-width: 18px;
                min-height: 18px;
                padding: 0;
                border-radius: 999px;
                border: 1px solid alpha(white, 0.16);
                box-shadow: none;
                transition: box-shadow 120ms ease, border-color 120ms ease;
            }

            button.editor-background-plain-color-button:hover {
                border-color: alpha(white, 0.32);
            }

            button.editor-background-plain-color-button:active {
                border-color: alpha(white, 0.42);
            }

            button.editor-background-plain-color-button.active-background-option {
                border-color: transparent;
                box-shadow: 0 0 0 2px #b05c38;
            }

            button.editor-background-plain-color-button.editor-background-plain-color-1 { background: #ffffff; }
            button.editor-background-plain-color-button.editor-background-plain-color-2 { background: #e5e7eb; }
            button.editor-background-plain-color-button.editor-background-plain-color-3 { background: #9ca3af; }
            button.editor-background-plain-color-button.editor-background-plain-color-4 { background: #111827; }
            button.editor-background-plain-color-button.editor-background-plain-color-5 { background: #ef4444; }
            button.editor-background-plain-color-button.editor-background-plain-color-6 { background: #f97316; }
            button.editor-background-plain-color-button.editor-background-plain-color-7 { background: #facc15; }
            button.editor-background-plain-color-button.editor-background-plain-color-8 { background: #22c55e; }
            button.editor-background-plain-color-button.editor-background-plain-color-9 { background: #14b8a6; }
            button.editor-background-plain-color-button.editor-background-plain-color-10 { background: #06b6d4; }
            button.editor-background-plain-color-button.editor-background-plain-color-11 { background: #3b82f6; }
            button.editor-background-plain-color-button.editor-background-plain-color-12 { background: #6366f1; }
            button.editor-background-plain-color-button.editor-background-plain-color-13 { background: #8b5cf6; }
            button.editor-background-plain-color-button.editor-background-plain-color-14 { background: #a855f7; }
            button.editor-background-plain-color-button.editor-background-plain-color-15 { background: #ec4899; }
            button.editor-background-plain-color-button.editor-background-plain-color-16 { background: #f43f5e; }
            button.editor-background-plain-color-button.editor-background-plain-color-17 { background: #92400e; }
            button.editor-background-plain-color-button.editor-background-plain-color-18 { background: #0f766e; }

            button.editor-background-gradient-button.editor-background-gradient-preview-1 {
                background-image: linear-gradient(135deg, #4f46e5 0%, #9333ea 55%, #ec4899 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-2 {
                background-image: linear-gradient(135deg, #0f172a 0%, #1d4ed8 48%, #38bdf8 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-3 {
                background-image: linear-gradient(135deg, #f97316 0%, #fb7185 52%, #a855f7 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-4 {
                background-image: linear-gradient(135deg, #134e4a 0%, #14b8a6 50%, #99f6e4 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-5 {
                background-image: linear-gradient(135deg, #111827 0%, #374151 45%, #f59e0b 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-6 {
                background-image: linear-gradient(135deg, #7c2d12 0%, #ea580c 50%, #fdba74 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-7 {
                background-image: linear-gradient(135deg, #052e16 0%, #16a34a 50%, #bbf7d0 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-8 {
                background-image: linear-gradient(135deg, #172554 0%, #2563eb 55%, #c4b5fd 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-9 {
                background-image: linear-gradient(135deg, #4a044e 0%, #c026d3 48%, #f9a8d4 100%);
            }

            button.editor-background-gradient-button.editor-background-gradient-preview-10 {
                background-image: linear-gradient(135deg, #3f3f46 0%, #71717a 40%, #e4e4e7 100%);
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

fn icon_stroke_color(widget: &DrawingArea) -> gdk::RGBA {
    widget
        .style_context()
        .lookup_color("theme_fg_color")
        .unwrap_or_else(|| gdk::RGBA::new(1.0, 1.0, 1.0, 1.0))
}

fn draw_arrow_icon(context: &gtk4::cairo::Context, width: f64, height: f64, style: ArrowStyle) {
    let start_x = width * 0.22;
    let start_y = height * 0.78;
    let end_x = width * 0.78;
    let end_y = height * 0.26;

    match style {
        ArrowStyle::Curved => {
            context.move_to(start_x, start_y);
            context.curve_to(
                width * 0.28,
                height * 0.24,
                width * 0.62,
                height * 0.82,
                end_x,
                end_y,
            );
            let _ = context.stroke();
            context.move_to(end_x - width * 0.18, end_y + height * 0.03);
            context.line_to(end_x, end_y);
            context.line_to(end_x - width * 0.05, end_y + height * 0.18);
        }
        ArrowStyle::Double => {
            context.move_to(start_x, start_y);
            context.line_to(end_x, end_y);
            let _ = context.stroke();
            context.move_to(start_x + width * 0.15, start_y - height * 0.01);
            context.line_to(start_x, start_y);
            context.line_to(start_x + width * 0.05, start_y - height * 0.16);
            context.move_to(end_x - width * 0.18, end_y + height * 0.03);
            context.line_to(end_x, end_y);
            context.line_to(end_x - width * 0.05, end_y + height * 0.18);
        }
        ArrowStyle::Fancy => {
            context.set_line_width(2.4);
            context.move_to(start_x, start_y);
            context.line_to(end_x - width * 0.08, end_y + height * 0.08);
            let _ = context.stroke();
            context.move_to(end_x - width * 0.22, end_y + height * 0.06);
            context.line_to(end_x, end_y);
            context.line_to(end_x - width * 0.08, end_y + height * 0.24);
        }
        ArrowStyle::Standard => {
            context.move_to(start_x, start_y);
            context.line_to(end_x, end_y);
            let _ = context.stroke();
            context.move_to(end_x - width * 0.18, end_y + height * 0.03);
            context.line_to(end_x, end_y);
            context.line_to(end_x - width * 0.05, end_y + height * 0.18);
        }
    }
}

fn custom_tool_icon_widget(icon: EditorToolIcon, size: i32) -> Widget {
    let area = DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_draw_func(move |widget, context, width, height| {
        let inset = custom_toolbar_icon_inset(&icon);
        let color = icon_stroke_color(widget);
        context.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            color.alpha() as f64,
        );
        context.set_line_width(1.55);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        context.set_line_join(gtk4::cairo::LineJoin::Round);

        let width = (width as f64 - (inset * 2.0)).max(1.0);
        let height = (height as f64 - (inset * 2.0)).max(1.0);
        let _ = context.save();
        context.translate(inset, inset);

        match icon {
            EditorToolIcon::ArrowStyle(style) => {
                draw_arrow_icon(context, width, height, style);
                let _ = context.stroke();
            }
            EditorToolIcon::Named(_) => {}
        }
        let _ = context.restore();
    });
    area.upcast::<Widget>()
}

pub fn tool_icon_widget(icon: EditorToolIcon, size: i32) -> Widget {
    match icon {
        EditorToolIcon::Named(icon_name) => {
            let image = Image::from_icon_name(&icon_name);
            image.set_pixel_size(size);
            image.upcast::<Widget>()
        }
        _ => custom_tool_icon_widget(icon, size),
    }
}

pub fn set_button_tool_icon(button: &Button, icon: EditorToolIcon, size: i32) {
    let child = tool_icon_widget(icon, size);
    button.set_child(Some(&child));
}

pub fn icon_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let image = tool_icon_widget(EditorToolIcon::Named(icon_name.to_owned()), 14);

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
    image.set_pixel_size(18);

    let button = Button::new();
    button.set_child(Some(&image));
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("editor-footer-icon-button");

    (button, image)
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

pub fn recommended_window_size_with_extra_width(
    image_width: i32,
    image_height: i32,
    extra_width: i32,
) -> (i32, i32) {
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
    let ui_width = 72.0 + extra_width.max(0) as f64;
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

#[allow(dead_code)]
pub fn recommended_window_size(image_width: i32, image_height: i32) -> (i32, i32) {
    recommended_window_size_with_extra_width(image_width, image_height, 0)
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

const EDITOR_EDGE_RESIZE_MARGIN: f64 = 8.0;

fn edge_cursor_name(x: f64, y: f64, width: f64, height: f64) -> Option<&'static str> {
    let left = x <= EDITOR_EDGE_RESIZE_MARGIN;
    let right = x >= width - EDITOR_EDGE_RESIZE_MARGIN;
    let top = y <= EDITOR_EDGE_RESIZE_MARGIN;
    let bottom = y >= height - EDITOR_EDGE_RESIZE_MARGIN;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some("nw-resize"),
        (false, true, true, false) => Some("ne-resize"),
        (true, false, false, true) => Some("sw-resize"),
        (false, true, false, true) => Some("se-resize"),
        (false, false, true, false) => Some("n-resize"),
        (false, false, false, true) => Some("s-resize"),
        (true, false, false, false) => Some("w-resize"),
        (false, true, false, false) => Some("e-resize"),
        _ => None,
    }
}

fn edge_for_resize(x: f64, y: f64, width: f64, height: f64) -> Option<gdk::SurfaceEdge> {
    let left = x <= EDITOR_EDGE_RESIZE_MARGIN;
    let right = x >= width - EDITOR_EDGE_RESIZE_MARGIN;
    let top = y <= EDITOR_EDGE_RESIZE_MARGIN;
    let bottom = y >= height - EDITOR_EDGE_RESIZE_MARGIN;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some(gdk::SurfaceEdge::NorthWest),
        (false, true, true, false) => Some(gdk::SurfaceEdge::NorthEast),
        (true, false, false, true) => Some(gdk::SurfaceEdge::SouthWest),
        (false, true, false, true) => Some(gdk::SurfaceEdge::SouthEast),
        (false, false, true, false) => Some(gdk::SurfaceEdge::North),
        (false, false, false, true) => Some(gdk::SurfaceEdge::South),
        (true, false, false, false) => Some(gdk::SurfaceEdge::West),
        (false, true, false, false) => Some(gdk::SurfaceEdge::East),
        _ => None,
    }
}

pub fn install_edge_resize(root: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    use gtk4::{EventControllerMotion, GestureClick};

    let resize_motion = EventControllerMotion::new();
    let window_resize_motion = window.downgrade();
    resize_motion.connect_motion(move |controller, x, y| {
        let Some(widget) = controller.widget() else {
            return;
        };
        let width = widget.allocated_width() as f64;
        let height = widget.allocated_height() as f64;
        let cursor = edge_cursor_name(x, y, width, height)
            .and_then(|name| gdk::Cursor::from_name(name, None));
        widget.set_cursor(cursor.as_ref());
        if cursor.is_none() {
            if let Some(window) = window_resize_motion.upgrade() {
                window.set_cursor(None);
            }
        }
    });

    let resize_motion_leave = EventControllerMotion::new();
    resize_motion_leave.connect_leave(move |controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });

    let resize_click = GestureClick::new();
    resize_click.set_button(1);
    let window_resize = window.downgrade();
    resize_click.connect_pressed(move |gesture, _, x, y| {
        let Some(window) = window_resize.upgrade() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let Some(event) = gesture.current_event() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let Some(device) = event.device() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let width = window.allocated_width() as f64;
        let height = window.allocated_height() as f64;
        let Some(edge) = edge_for_resize(x, y, width, height) else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let Some(surface) = window.surface() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            x,
            y,
            event.time(),
        );
    });

    root.add_controller(resize_motion);
    root.add_controller(resize_motion_leave);
    root.add_controller(resize_click);
}

pub fn install_window_drag(toolbar: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    use gtk4::GestureClick;

    let drag_window_gesture = GestureClick::new();
    drag_window_gesture.set_button(0); // Accept any button
    let window_drag = window.downgrade();
    drag_window_gesture.connect_pressed(move |gesture, _, x, y| {
        // Check if the click was on an interactive widget (button, entry, etc.)
        let Some(widget) = gesture.widget() else {
            return;
        };

        // Get the widget at the click position
        let picked = widget.pick(x, y, gtk4::PickFlags::DEFAULT);

        // Only drag if clicked on empty space (not on buttons or other interactive widgets)
        if let Some(picked) = picked {
            let type_name = picked.type_().name();
            if type_name == "GtkButton"
                || type_name == "GtkToggleButton"
                || type_name == "GtkMenuButton"
                || type_name == "GtkEntry"
                || type_name == "GtkScale"
                || type_name == "GtkPopover"
            {
                return; // Don't drag if clicked on interactive widget
            }

            // Also check for buttons inside boxes
            if let Some(parent) = picked.parent() {
                let parent_type = parent.type_().name();
                if parent_type == "GtkButton"
                    || parent_type == "GtkToggleButton"
                    || parent_type == "GtkMenuButton"
                {
                    return;
                }
            }
        }

        let Some(window) = window_drag.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    toolbar.add_controller(drag_window_gesture);
}

#[cfg(test)]
mod tests {
    use super::{
        arrow_style_toolbar_icon, custom_toolbar_icon_inset, toolbar_icon_size, EditorToolIcon,
    };
    use crate::capture::editor::types::ArrowStyle;

    #[test]
    fn arrow_style_toolbar_icon_maps_each_style_to_a_custom_preview() {
        for style in ArrowStyle::ALL {
            assert_eq!(
                arrow_style_toolbar_icon(style),
                EditorToolIcon::ArrowStyle(style)
            );
        }
    }

    #[test]
    fn toolbar_icon_size_uses_the_same_box_for_named_and_custom_icons() {
        assert_eq!(
            toolbar_icon_size(&EditorToolIcon::ArrowStyle(ArrowStyle::Curved)),
            14
        );
        assert_eq!(
            toolbar_icon_size(&EditorToolIcon::Named("fallback".to_owned())),
            14
        );
    }

    #[test]
    fn custom_toolbar_icons_use_internal_padding_to_match_stock_icon_optics() {
        assert!(custom_toolbar_icon_inset(&EditorToolIcon::ArrowStyle(ArrowStyle::Curved)) > 0.0);
        assert_eq!(
            custom_toolbar_icon_inset(&EditorToolIcon::Named("fallback".to_owned())),
            0.0
        );
    }

    #[test]
    fn editor_css_avoids_unsupported_gtk_properties() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        for property in [
            "align-items:",
            "text-align:",
            "\n                height:",
            "\n                width:",
            "max-width:",
            "overflow:",
            "\n                spacing:",
        ] {
            assert!(
                !production_source.contains(property),
                "editor CSS still contains unsupported GTK property {property}"
            );
        }
    }

    #[test]
    fn editor_background_slider_css_does_not_override_internal_contents_nodes() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains(".editor-background-padding-slider > contents")
                && !production_source.contains(".editor-background-compact-slider > contents"),
            "editor background slider CSS still overrides internal GTK contents nodes"
        );
    }

    #[test]
    fn editor_background_alignment_css_uses_larger_button_and_marker_sizes() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("min-height: 24px;")
                && production_source.contains("min-width: 34px;")
                && production_source.contains("min-width: 10px;")
                && production_source.contains("min-height: 6px;"),
            "alignment CSS should keep the larger button shell and marker sizes",
        );
    }

    #[test]
    fn editor_background_alignment_active_state_uses_orange_accent() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("button.editor-background-alignment-button.active-alignment-option {")
                && production_source.contains("box-shadow: inset 0 0 0 1px #b05c38;"),
            "alignment selected state should use the #B05C38 editor accent",
        );
    }

    #[test]
    fn editor_toolbar_active_tool_uses_flat_white_alpha_matching_settings_nav() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("button.editor-tool-button.active-tool {")
                && production_source.contains("background-color: alpha(white, 0.14);")
                && production_source.contains("border: none;"),
            "selected annotate toolbar tools should use a flat alpha(white, 0.14) background matching settings-nav-item-selected pattern",
        );
    }

    #[test]
    fn inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("button.editor-inspector-tab-button {\n                min-height: 20px;\n                padding: 0;\n                border-radius: 0;\n                border: none;\n                background: transparent;")
                && production_source.contains("button.editor-inspector-tab-button.active-inspector-tab {\n                background: transparent;\n                border: none;\n                color: #b05c38;")
                && production_source.contains(".editor-inspector-tabs {\n                margin-top: 16px;\n                margin-bottom: 12px;")
                && production_source.contains(".editor-right-inspector {\n                min-width: 210px;")
                && production_source.contains(".editor-inspector-placeholder-shell {\n                min-width: 210px;")
                && production_source.contains(".editor-background-sidebar {\n                min-width: 210px;")
                && production_source.contains(".editor-colors-panel {\n                min-width: 210px;")
                && !production_source.contains(".editor-background-sidebar {\n                min-width: 210px;\n                width: 210px;")
                && !production_source.contains(".editor-colors-panel {\n                min-width: 210px;\n                width: 210px;"),
            "inspector tabs should be text-only, with a fixed shell width but flexible inner panel surfaces",
        );
    }

    #[test]
    fn arrow_inspector_active_option_uses_subtle_surface_and_orange_tick() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-arrow-inspector-option-active")
                && production_source.contains(".editor-arrow-inspector-check,\n            .editor-text-inspector-check,\n            .editor-obfuscate-inspector-check,\n            .editor-number-style-check,\n            .editor-number-size-check {\n                color: #b05c38;"),
            "Arrow inspector selection should use a subtle row surface plus an orange tick indicator",
        );
    }

    #[test]
    fn text_inspector_option_rows_match_arrow_visual_language_without_changing_panel_width() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-text-inspector-option-active")
                && production_source.contains(".editor-arrow-inspector-check,\n            .editor-text-inspector-check,\n            .editor-obfuscate-inspector-check,\n            .editor-number-style-check,\n            .editor-number-size-check {\n                color: #b05c38;")
                && production_source.contains(".editor-right-inspector {\n                min-width: 210px;")
                && !production_source.contains("TEXT_SIDEBAR_WIDTH"),
            "Text inspector rows should mirror Arrow selection styling without introducing a new sidepanel width path",
        );
    }

    #[test]
    fn obfuscate_inspector_option_rows_match_migrated_tool_panels_without_new_width_path() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-obfuscate-inspector-option-active")
                && production_source.contains(".editor-arrow-inspector-check,\n            .editor-text-inspector-check,\n            .editor-obfuscate-inspector-check,\n            .editor-number-style-check,\n            .editor-number-size-check {\n                color: #b05c38;")
                && production_source.contains(".editor-right-inspector {\n                min-width: 210px;")
                && !production_source.contains("OBFUSCATE_SIDEBAR_WIDTH"),
            "Obfuscate inspector rows should use the shared sidepanel language without introducing a new width path",
        );
    }

    #[test]
    fn number_inspector_rows_match_migrated_sidepanel_surface_language_without_new_width_path() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-number-style-option-active")
                && production_source.contains("editor-number-size-option-active")
                && production_source.contains(".editor-arrow-inspector-check,\n            .editor-text-inspector-check,\n            .editor-obfuscate-inspector-check,\n            .editor-number-style-check,\n            .editor-number-size-check {\n                color: #b05c38;")
                && production_source.contains(".editor-right-inspector {\n                min-width: 210px;")
                && !production_source.contains("NUMBER_SIDEBAR_WIDTH"),
            "Number inspector rows should match the migrated sidepanel surface language without introducing a new width path",
        );
    }

    #[test]
    fn number_inspector_start_controls_use_sidebar_field_styling() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(".editor-number-start-label {")
                && production_source.contains(".editor-number-start-entry {")
                && production_source.contains("button.editor-number-start-stepper {"),
            "Number inspector start controls should be styled as inspector-native sidebar fields",
        );
    }

    #[test]
    fn crop_inspector_option_and_dimensions_styles_use_existing_inspector_surface_language() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-crop-inspector-option-active")
                && production_source.contains(".editor-crop-inspector-check {\n                color: #b05c38;")
                && production_source.contains(".editor-crop-dimensions-row {\n                padding: 12px 0;"),
            "Crop inspector should use the same restrained inspector surface language as the other side-panel tools",
        );
    }

    #[test]
    fn footer_zoom_popover_reuses_inspector_surface_language_without_sidebar_dimensions() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(".editor-footer-zoom-popup {")
                && production_source.contains("background: #1a1a1a;")
                && production_source.contains("border-radius: 10px;")
                && !production_source.contains(".editor-footer-zoom-popup {\n                min-height: 100%;")
                && !production_source.contains(".editor-footer-zoom-popup {\n                width: 210px;"),
            "Footer zoom popover should use a flat surface matching the settings UI language without becoming a full-height or fixed-width sidebar",
        );
    }

    #[test]
    fn footer_zoom_rows_distinguish_actions_from_instructional_rows() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("button.editor-footer-zoom-action-btn {")
                && production_source.contains("button.editor-footer-zoom-action-btn:hover {")
                && production_source.contains(".editor-footer-zoom-row {"),
            "Footer zoom styling should keep action rows interactive and hint rows clearly non-interactive",
        );
    }

    #[test]
    fn add_to_colors_button_uses_orange_editor_accent() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("button.editor-add-to-colors-button {")
                && production_source.contains("background: #B05C38;")
                && production_source.contains("button.editor-add-to-colors-button:hover {\n                background: #C66B4A;")
                && production_source.contains("button.editor-add-to-colors-button:active {\n                background: #8A4A2D;"),
            "Add to colors button should use the #B05C38 editor accent states",
        );
    }

    #[test]
    fn toolbar_color_status_chip_has_swatch_and_hex_styles() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(".editor-toolbar-color-status {")
                && production_source.contains(".editor-toolbar-color-status-swatch {")
                && production_source.contains(".editor-toolbar-color-status-label {"),
            "Toolbar color status chip should have dedicated swatch and label styles",
        );
    }

    #[test]
    fn editor_toolbar_slider_css_does_not_style_internal_slider_nodes() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains(".editor-toolbar-size-slider trough")
                && !production_source.contains(".editor-toolbar-size-slider highlight")
                && !production_source.contains(".editor-toolbar-size-slider slider"),
            "editor toolbar slider CSS still styles internal GTK slider nodes"
        );
    }

    #[test]
    fn editor_opacity_slider_css_does_not_style_internal_slider_nodes() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains(".editor-opacity-slider trough")
                && !production_source.contains(".editor-opacity-slider highlight")
                && !production_source.contains(".editor-opacity-slider slider"),
            "editor opacity slider CSS still styles internal GTK slider nodes"
        );
    }
}
