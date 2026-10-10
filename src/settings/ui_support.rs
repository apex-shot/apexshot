use gtk4::gdk;
use gtk4::{prelude::*, Button, CssProvider};

const SETTINGS_CSS: &str = concat!(
    include_str!("css/01-shell-controls.css"),
    include_str!("css/02-sidebar-actions.css"),
    include_str!("css/03-shared-content.css"),
    include_str!("css/04-native-widgets.css"),
    include_str!("css/05-tabs-modes-shortcuts.css"),
    include_str!("css/06-shortcut-dialog.css"),
    include_str!("css/07-pages-about-onboarding.css"),
    include_str!("css/08-recent-captures.css"),
    include_str!("css/09-noir-gallery.css"),
    include_str!("css/10-history-shell.css"),
    include_str!("css/11-theme-picker.css"),
);

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
        button.add_css_class("recording-editor-traffic-close");
    }
    if let Some(image) = button.child().and_downcast::<gtk4::Image>() {
        image.set_pixel_size(16);
    }
    button.set_size_request(28, 28);

    button
}

#[cfg(test)]
mod tests {
    use super::SETTINGS_CSS;

    #[test]
    fn settings_css_parses_without_gtk_errors() {
        // CSS providers are GTK objects, so this runs on the test binary's
        // shared GTK thread (see `crate::test_support`).
        let Some((errors, bogus_errors)) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            {
                let errors = std::rc::Rc::clone(&errors);
                provider.connect_parsing_error(move |_, section, error| {
                    errors
                        .borrow_mut()
                        .push(format!("{}: {}", section.to_str(), error.message()));
                });
            }
            provider.load_from_data(SETTINGS_CSS);

            // Harness sanity check: a bogus declaration must be reported,
            // otherwise the assertion below could pass because the signal
            // never fires.
            let bogus = gtk4::CssProvider::new();
            let bogus_errors = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            {
                let bogus_errors = std::rc::Rc::clone(&bogus_errors);
                bogus.connect_parsing_error(move |_, _, error| {
                    bogus_errors.borrow_mut().push(error.message().to_string());
                });
            }
            bogus.load_from_data(".bogus { definitely-not-a-property: 1; }");

            // Bind before returning: the `Ref` guards have to drop before the
            // `Rc`s they borrow from.
            let parsed: Vec<String> = errors.borrow().clone();
            let reported: Vec<String> = bogus_errors.borrow().clone();
            (parsed, reported)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert!(
            errors.is_empty(),
            "settings CSS failed to parse: {errors:#?}"
        );
        assert!(
            !bogus_errors.is_empty(),
            "parsing-error signal is not firing; the CSS check is vacuous"
        );
    }

    #[test]
    fn settings_css_avoids_unsupported_gtk_properties() {
        for property in ["max-width", "overflow", "backdrop-filter"] {
            assert!(
                !SETTINGS_CSS.contains(property),
                "settings CSS still contains unsupported GTK property: {property}"
            );
        }
    }

    #[test]
    fn settings_window_controls_inherit_the_root_surface() {
        assert!(SETTINGS_CSS.contains(
            ".settings-window-controls {\n                min-height: 0;\n                padding: 0;\n                background: transparent;"
        ));
        assert!(SETTINGS_CSS.contains(
            ".editor-root.editor-theme-light .settings-window-controls {\n                background: transparent;"
        ));
    }
}
