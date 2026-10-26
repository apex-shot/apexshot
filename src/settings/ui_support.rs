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

/// Round window-control pill for the platforms that keep traffic-light
/// title bars. The Windows windows draw caption buttons instead.
#[cfg(not(target_os = "windows"))]
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

/// Height of a custom title bar at 100% scale.
///
/// Windows packs the bar down to the system caption height; the other platforms
/// keep the taller shell the sidebar head is aligned with.
pub const TITLE_BAR_HEIGHT: f64 = if cfg!(target_os = "windows") {
    32.0
} else {
    52.0
};

/// Width of one caption target in a Windows title bar, at 100% scale.
#[cfg(target_os = "windows")]
pub const CAPTION_BUTTON_WIDTH: f64 = 46.0;

/// The design box the caption strokes are drawn in, at 100% scale.
#[cfg(target_os = "windows")]
pub const CAPTION_GLYPH_SIZE: f64 = 10.0;

/// The glyph a Windows caption button paints.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptionGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// A window control in one of the app's custom title bars.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowControl {
    Minimize,
    Close,
}

/// Build the button for `role` in a custom title bar.
///
/// Windows titles use the system's flat, full-height caption targets; the other
/// platforms keep the round traffic-light pills.
pub fn window_control_button(role: WindowControl, tooltip: &str, scale: f64) -> Button {
    #[cfg(target_os = "windows")]
    {
        let glyph = match role {
            WindowControl::Minimize => CaptionGlyph::Minimize,
            WindowControl::Close => CaptionGlyph::Close,
        };
        let (button, _) = caption_button(
            std::rc::Rc::new(std::cell::Cell::new(glyph)),
            tooltip,
            scale,
        );
        button
    }

    #[cfg(not(target_os = "windows"))]
    {
        let color_class = match role {
            WindowControl::Minimize => "traffic-light-yellow",
            WindowControl::Close => "traffic-light-red",
        };
        let button = traffic_light_button(color_class, tooltip);
        button.remove_css_class("recent-captures-wm-btn");
        button.remove_css_class("recent-captures-wm-close");
        button.add_css_class("recording-editor-traffic-btn");
        let size = (28.0 * scale).round() as i32;
        button.set_size_request(size, size);
        button.set_valign(gtk4::Align::Center);
        button
    }
}

/// A flat caption button, sized and laid out like the system's own window
/// controls: 46px wide, full title-bar height, with a hand-drawn glyph.
///
/// The drawn area is returned so a window can repaint the glyph when the state
/// it shows changes (maximize becoming restore).
#[cfg(target_os = "windows")]
pub fn caption_button(
    glyph: std::rc::Rc<std::cell::Cell<CaptionGlyph>>,
    tooltip: &str,
    scale: f64,
) -> (Button, gtk4::DrawingArea) {
    let button = Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("caption-btn");
    if glyph.get() == CaptionGlyph::Close {
        button.add_css_class("caption-btn-close");
    }
    button.set_size_request(
        (CAPTION_BUTTON_WIDTH * scale).round() as i32,
        (TITLE_BAR_HEIGHT * scale).round() as i32,
    );

    let glyph_size = (CAPTION_GLYPH_SIZE * scale).round().max(2.0);
    let area = gtk4::DrawingArea::new();
    area.set_content_width(glyph_size as i32);
    area.set_content_height(glyph_size as i32);
    area.set_halign(gtk4::Align::Center);
    area.set_valign(gtk4::Align::Center);
    area.set_can_target(false);
    area.set_draw_func(move |area, context, width, height| {
        draw_caption_glyph(
            context,
            f64::from(width),
            f64::from(height),
            glyph_size,
            glyph.get(),
            area.style_context().color(),
        );
    });

    // The glyph's colour comes from the button's CSS `color`, so repaint when
    // hover changes it (notably the white X over the close button's red fill).
    let motion = gtk4::EventControllerMotion::new();
    {
        let area = area.clone();
        motion.connect_enter(move |_, _, _| area.queue_draw());
    }
    {
        let area = area.clone();
        motion.connect_leave(move |_| area.queue_draw());
    }
    button.add_controller(motion);

    button.set_child(Some(&area));
    (button, area)
}

#[cfg(target_os = "windows")]
fn draw_caption_glyph(
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    glyph_size: f64,
    glyph: CaptionGlyph,
    color: gdk::RGBA,
) {
    let size = glyph_size.max(1.0);
    context.save().ok();
    context.translate((width - size) * 0.5, (height - size) * 0.5);
    context.scale(size / CAPTION_GLYPH_SIZE, size / CAPTION_GLYPH_SIZE);
    context.set_line_width(1.0);
    context.set_line_cap(gtk4::cairo::LineCap::Square);
    context.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );

    // Half-pixel insets keep the one-pixel strokes crisp at 100% scaling.
    let near = 0.5;
    let far = CAPTION_GLYPH_SIZE - 0.5;
    let middle = CAPTION_GLYPH_SIZE / 2.0;
    match glyph {
        CaptionGlyph::Minimize => {
            context.move_to(near, middle);
            context.line_to(far, middle);
        }
        CaptionGlyph::Maximize => {
            context.rectangle(near, near, far - near, far - near);
        }
        CaptionGlyph::Restore => {
            let offset = 2.5;
            let small = far - near - offset;
            context.rectangle(near + offset, near, small, small);
            context.rectangle(near, near + offset, small, small);
        }
        CaptionGlyph::Close => {
            context.move_to(near, near);
            context.line_to(far, far);
            context.move_to(far, near);
            context.line_to(near, far);
        }
    }
    context.stroke().ok();
    context.restore().ok();
}

#[cfg(all(test, unix))]
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
