use gtk4::gdk;
use gtk4::{prelude::*, Button, CssProvider};

const SETTINGS_CSS: &str = include_str!("settings.css");

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
    button.set_size_request(24, 24);

    button
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
