use gtk4::{gdk, CssProvider};

const RECORDING_EDITOR_CSS: &str = concat!(
    "\n",
    include_str!("ui_support_css/01.css"),
    include_str!("ui_support_css/02.css"),
    include_str!("ui_support_css/03.css"),
    include_str!("ui_support_css/04.css"),
    include_str!("ui_support_css/05.css"),
    include_str!("ui_support_css/06.css"),
    include_str!("ui_support_css/07.css"),
    include_str!("ui_support_css/08.css"),
    include_str!("ui_support_css/09.css"),
);

pub fn install_recording_editor_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(RECORDING_EDITOR_CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RECORDING_EDITOR_CSS;

    #[test]
    fn light_theme_chrome_matches_timeline() {
        assert!(RECORDING_EDITOR_CSS.contains(
            ".editor-theme-light .recording-editor-window-controls {\n                background: #ffffff;"
        ));
        assert!(RECORDING_EDITOR_CSS.contains(
            ".editor-theme-light.recording-editor-shell {\n                background: #ffffff;\n                border: 1px solid alpha(#111827, 0.18);"
        ));
        assert!(RECORDING_EDITOR_CSS
            .contains(".recording-editor-root scale slider {\n                min-width: 12px;"));
        assert!(RECORDING_EDITOR_CSS
            .contains(".recording-editor-root scale.recording-editor-timeline-zoom slider"));
        assert!(RECORDING_EDITOR_CSS.contains("box-shadow: none;"));
        assert!(RECORDING_EDITOR_CSS.contains(
            ".recording-editor-root scrollbar.vertical {\n                min-width: 6px;"
        ));
        assert!(RECORDING_EDITOR_CSS.contains(
            ".recording-editor-root scrollbar slider {\n                background-color: alpha(white, 0.18);\n                border-radius: 999px;\n                min-width: 5px;"
        ));
    }

    #[test]
    fn the_custom_wallpaper_picker_css_parses_without_gtk_errors() {
        // A string match proves the surface rules are present, not that GTK
        // accepts them. This loads the picker's slice of the real stylesheet
        // into a CssProvider so a mistyped property on the card is caught here
        // rather than as a silently-ignored declaration at runtime. Mirrors the
        // settings stylesheet's check, including its bogus-declaration sanity
        // probe — without that probe this assertion could pass because the
        // error signal never fires.
        //
        // Scoped to the picker's sections, not the whole bundle: 01-08 carry 23
        // pre-existing `max-width`/`overflow` warnings in files the
        // `light_theme_chrome_matches_timeline` test pins byte for byte, so
        // asserting the whole stylesheet is clean would fail for reasons this
        // test cannot fix.
        const START: &str = "/* ── Custom Wallpaper popover ──";
        const END: &str = "/* ── Light theme ── */";
        let css = include_str!("ui_support_css/09.css");
        let start = css
            .find(START)
            .expect("09.css must have a Custom Wallpaper popover section");
        let end = css[start..]
            .find(END)
            .map(|at| start + at)
            .expect("the popover section must be followed by the light theme");
        let slice = format!("{}\n{}\n", &css[start..end], &css[end..]);

        let Some((errors, bogus_errors)) = crate::test_support::with_gtk(move || {
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
            provider.load_from_data(&slice);

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
            "the Custom Wallpaper picker CSS failed to parse: {errors:#?}"
        );
        assert!(
            !bogus_errors.is_empty(),
            "parsing-error signal is not firing; the CSS check is vacuous"
        );
    }
}
