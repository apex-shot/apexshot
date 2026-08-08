use super::types::ArrowStyle;
use gtk4::gdk;
use gtk4::{
    prelude::*, Box as GtkBox, Button, CssProvider, DrawingArea, Image, Orientation, Widget,
};
use std::process::Command;

pub const EDITOR_MIN_WINDOW_WIDTH: i32 = 980;

/// Reserved strip above the canvas for the floating toolbar + window drag.
/// The image viewport starts below this so zoomed content cannot cover the tools.
pub const EDITOR_TOP_CHROME_HEIGHT: i32 = 56;

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
        if color_scheme.contains("prefer-light") || color_scheme == "default" {
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

const EDITOR_CSS: &str = include_str!("editor.css");

pub fn install_editor_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(EDITOR_CSS);
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
    // Keep focus on the canvas so Space continues to pan after tool clicks.
    button.set_focusable(false);
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
    button.set_focusable(false);
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
    button.set_size_request(24, 24);

    button
}

pub fn recommended_window_size_with_extra_width(
    _image_width: i32,
    _image_height: i32,
    extra_width: i32,
) -> (i32, i32) {
    // Keep the editor window size stable. Do not auto-resize the window based on
    // the opened screenshot dimensions; users can manually resize the editor if
    // they want a larger or smaller workspace.
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

    let max_width = ((screen_width as f64) * 0.90).round() as i32;
    let max_height = ((screen_height as f64) * 0.85).round() as i32;
    let default_width = 1280 + extra_width.max(0);
    let default_height = 820;

    (
        default_width.clamp(EDITOR_MIN_WINDOW_WIDTH.min(max_width), max_width.max(1)),
        default_height.clamp(560.min(max_height), max_height.max(1)),
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

fn is_interactive_widget(widget: &gtk4::Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(widget) = current {
        // Canvas lives under the floating top bar after the redesign. Treat the
        // whole canvas subtree as interactive so the top-64px window-drag
        // handler does not call begin_move() over DrawingArea hits.
        if widget.has_css_class("editor-canvas")
            || widget.has_css_class("editor-canvas-frame")
            || widget.has_css_class("editor-canvas-workspace")
            || widget.has_css_class("editor-floating-zoom")
            || widget.has_css_class("editor-floating-history")
        {
            return true;
        }

        if matches!(
            widget.type_().name(),
            "GtkButton"
                | "GtkToggleButton"
                | "GtkMenuButton"
                | "GtkEntry"
                | "GtkScale"
                | "GtkDropDown"
                | "GtkCheckButton"
                | "GtkPopover"
                | "GtkDrawingArea"
                | "GtkScrolledWindow"
                | "GtkViewport"
                | "GtkText"
                | "GtkTextView"
        ) {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn install_window_drag_in_region(
    widget: &impl IsA<gtk4::Widget>,
    window: &gtk4::ApplicationWindow,
    max_y: Option<f64>,
) {
    use gtk4::GestureClick;

    let drag_window_gesture = GestureClick::new();
    drag_window_gesture.set_button(1);
    let window_drag = window.downgrade();
    drag_window_gesture.connect_pressed(move |gesture, _, x, y| {
        if max_y.is_some_and(|max_y| y > max_y) {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        }

        let Some(widget) = gesture.widget() else {
            return;
        };
        let picked = widget.pick(x, y, gtk4::PickFlags::DEFAULT);
        if picked.as_ref().is_some_and(is_interactive_widget) {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        }

        let Some(window) = window_drag.upgrade() else {
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
        let Some(surface) = window.surface() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        };
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    widget.add_controller(drag_window_gesture);
}

pub fn install_window_drag(toolbar: &impl IsA<gtk4::Widget>, window: &gtk4::ApplicationWindow) {
    install_window_drag_in_region(toolbar, window, None);
}

pub fn install_top_bar_window_drag(
    root: &impl IsA<gtk4::Widget>,
    window: &gtk4::ApplicationWindow,
) {
    install_window_drag_in_region(root, window, Some(f64::from(EDITOR_TOP_CHROME_HEIGHT)));
}

#[cfg(test)]
mod tests {
    use super::{
        arrow_style_toolbar_icon, custom_toolbar_icon_inset, toolbar_icon_size, EditorToolIcon,
    };
    use crate::capture::editor::types::ArrowStyle;

    #[test]
    fn window_drag_hit_test_treats_canvas_as_interactive() {
        let source = include_str!("ui_support.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("\"GtkDrawingArea\"")
                && production_source.contains("\"GtkScrolledWindow\"")
                && production_source.contains("editor-canvas-frame")
                && production_source.contains("editor-canvas-workspace")
                && production_source.contains("EDITOR_TOP_CHROME_HEIGHT")
                && super::EDITOR_CSS.contains(".editor-top-chrome"),
            "top-bar window drag must not begin_move over the canvas subtree"
        );
    }

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
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
        assert!(
            !production_source.contains(".editor-background-padding-slider > contents")
                && !production_source.contains(".editor-background-compact-slider > contents"),
            "editor background slider CSS still overrides internal GTK contents nodes"
        );
    }

    #[test]
    fn editor_background_alignment_css_uses_larger_button_and_marker_sizes() {
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source
                .contains("button.editor-background-alignment-button.active-alignment-option {")
                && production_source.contains("box-shadow: inset 0 0 0 1px #b05c38;"),
            "alignment selected state should use the #B05C38 editor accent",
        );
    }

    #[test]
    fn editor_toolbar_active_tool_uses_flat_white_alpha_matching_settings_nav() {
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains("button.editor-tool-button.active-tool {")
                && production_source.contains("background-color: alpha(white, 0.14);")
                && production_source.contains("border: none;"),
            "selected annotate toolbar tools should use a flat alpha(white, 0.14) background matching settings-nav-item-selected pattern",
        );
    }

    #[test]
    fn inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width() {
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains("editor-arrow-inspector-option-active")
                && production_source.contains(".editor-arrow-inspector-check,\n            .editor-text-inspector-check,\n            .editor-obfuscate-inspector-check,\n            .editor-number-style-check,\n            .editor-number-size-check {\n                color: #b05c38;"),
            "Arrow inspector selection should use a subtle row surface plus an orange tick indicator",
        );
    }

    #[test]
    fn text_inspector_option_rows_match_arrow_visual_language_without_changing_panel_width() {
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains(".editor-number-start-label {")
                && production_source.contains(".editor-number-start-entry {")
                && production_source.contains("button.editor-number-start-stepper {"),
            "Number inspector start controls should be styled as inspector-native sidebar fields",
        );
    }

    #[test]
    fn crop_inspector_option_and_dimensions_styles_use_existing_inspector_surface_language() {
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains("editor-crop-inspector-option-active")
                && production_source.contains(".editor-crop-inspector-check {\n                color: #b05c38;")
                && production_source.contains(".editor-crop-dimensions-row {\n                padding: 12px 0;"),
            "Crop inspector should use the same restrained inspector surface language as the other side-panel tools",
        );
    }

    #[test]
    fn footer_zoom_popover_reuses_inspector_surface_language_without_sidebar_dimensions() {
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains(".editor-footer-zoom-popup {")
                && production_source.contains("background: #1a1a1a;")
                && production_source.contains("border-radius: 10px;")
                && production_source.contains(".editor-root.editor-theme-light .editor-footer-zoom-popup {")
                && production_source.contains(".editor-footer-zoom-popup.editor-theme-light {")
                && production_source.contains("background: #ffffff;")
                && production_source.contains(".editor-footer-zoom-popup.editor-theme-light .editor-footer-zoom-row {")
                && !production_source.contains(".editor-footer-zoom-popup {\n                min-height: 100%;")
                && !production_source.contains(".editor-footer-zoom-popup {\n                width: 210px;"),
            "Footer zoom popover should use flat dark/light surfaces matching the settings UI language without becoming a full-height or fixed-width sidebar",
        );
    }

    #[test]
    fn footer_zoom_rows_distinguish_actions_from_instructional_rows() {
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains("button.editor-footer-zoom-action-btn {")
                && production_source.contains("button.editor-footer-zoom-action-btn:hover {")
                && production_source.contains(".editor-footer-zoom-row {"),
            "Footer zoom styling should keep action rows interactive and hint rows clearly non-interactive",
        );
    }

    #[test]
    fn add_to_colors_button_uses_orange_editor_accent() {
        let production_source = super::EDITOR_CSS;
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
        let production_source = super::EDITOR_CSS;
        assert!(
            production_source.contains(".editor-toolbar-color-status {")
                && production_source.contains(".editor-toolbar-color-status-swatch {")
                && production_source.contains(".editor-toolbar-color-status-label {"),
            "Toolbar color status chip should have dedicated swatch and label styles",
        );
    }

    #[test]
    fn editor_toolbar_slider_css_does_not_style_internal_slider_nodes() {
        let production_source = super::EDITOR_CSS;
        assert!(
            !production_source.contains(".editor-toolbar-size-slider trough")
                && !production_source.contains(".editor-toolbar-size-slider highlight")
                && !production_source.contains(".editor-toolbar-size-slider slider"),
            "editor toolbar slider CSS still styles internal GTK slider nodes"
        );
    }

    #[test]
    fn editor_opacity_slider_css_does_not_style_internal_slider_nodes() {
        let production_source = super::EDITOR_CSS;
        assert!(
            !production_source.contains(".editor-opacity-slider trough")
                && !production_source.contains(".editor-opacity-slider highlight")
                && !production_source.contains(".editor-opacity-slider slider"),
            "editor opacity slider CSS still styles internal GTK slider nodes"
        );
    }

    #[test]
    fn editor_scale_css_does_not_style_internal_slider_nodes() {
        let production_source = super::EDITOR_CSS;
        assert!(
            !production_source.contains("scale slider"),
            "editor CSS should not style GTK scale slider nodes"
        );
    }
}
