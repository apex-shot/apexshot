//! Concrete editor window chrome: top drag strip, floating controls, edge resize (PR 10.14).
//!
//! Owns top-chrome construction over the canvas, floating zoom/history overlays,
//! and window drag/resize controller installation. Setup still owns workspace
//! assembly and session bootstrap.

use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Orientation, Overlay, Stack,
};

use super::super::ui_support::{
    install_edge_resize, install_top_bar_window_drag, install_window_drag, EDITOR_TOP_CHROME_HEIGHT,
};
use super::motion_mode::{MOTION_PAGE, STATIC_PAGE};

pub(super) struct WindowChromeInputs<'a> {
    pub canvas_with_toolbar: &'a Overlay,
    pub root_overlay: &'a Overlay,
    pub window: &'a ApplicationWindow,
    pub toolbar: &'a GtkBox,
    pub static_toolbar: &'a GtkBox,
    pub zoom_minus_btn: &'a Button,
    pub zoom_button: &'a Button,
    pub zoom_plus_btn: &'a Button,
    pub motion_btn: &'a Button,
    pub history_group: &'a GtkBox,
    pub zoom_popup: &'a GtkBox,
}

pub(super) struct WindowChrome {
    pub mode_stack: Stack,
    pub bottom_left_stack: Stack,
    pub motion_control: GtkBox,
    pub history_control: GtkBox,
}

/// Build top chrome + floating controls and install window drag/resize handlers.
pub(super) fn install_window_chrome(input: WindowChromeInputs<'_>) -> WindowChrome {
    let WindowChromeInputs {
        canvas_with_toolbar,
        root_overlay,
        window,
        toolbar,
        static_toolbar,
        zoom_minus_btn,
        zoom_button,
        zoom_plus_btn,
        motion_btn,
        history_group,
        zoom_popup,
    } = input;

    // Transparent full-width strip over the checkerboard: drag the window + host toolbar.
    let top_chrome = GtkBox::new(Orientation::Horizontal, 0);
    top_chrome.add_css_class("editor-top-chrome");
    top_chrome.set_halign(Align::Fill);
    top_chrome.set_valign(Align::Start);
    top_chrome.set_hexpand(true);
    top_chrome.set_vexpand(false);
    top_chrome.set_size_request(-1, EDITOR_TOP_CHROME_HEIGHT);
    top_chrome.set_can_target(true);

    let top_chrome_left = GtkBox::new(Orientation::Horizontal, 0);
    top_chrome_left.set_hexpand(true);
    let top_chrome_right = GtkBox::new(Orientation::Horizontal, 0);
    top_chrome_right.set_hexpand(true);

    let mode_stack = Stack::new();
    mode_stack.set_hhomogeneous(false);
    mode_stack.set_vhomogeneous(false);
    mode_stack.set_halign(Align::Center);
    mode_stack.set_valign(Align::Start);
    mode_stack.set_margin_top(8);
    mode_stack.set_hexpand(false);
    toolbar.set_halign(Align::Center);
    toolbar.set_valign(Align::Start);
    toolbar.set_hexpand(false);
    static_toolbar.set_halign(Align::Center);
    static_toolbar.set_valign(Align::Start);
    static_toolbar.set_hexpand(false);
    mode_stack.add_named(toolbar, Some(STATIC_PAGE));
    mode_stack.add_named(static_toolbar, Some(MOTION_PAGE));
    mode_stack.set_visible_child_name(STATIC_PAGE);

    top_chrome.append(&top_chrome_left);
    top_chrome.append(&mode_stack);
    top_chrome.append(&top_chrome_right);
    canvas_with_toolbar.add_overlay(&top_chrome);

    let zoom_control = GtkBox::new(Orientation::Horizontal, 0);
    zoom_control.add_css_class("editor-floating-zoom");
    zoom_control.append(zoom_minus_btn);
    zoom_control.append(zoom_button);
    zoom_control.append(zoom_plus_btn);

    let bottom_left_stack = Stack::new();
    bottom_left_stack.set_hhomogeneous(false);
    bottom_left_stack.set_vhomogeneous(false);
    bottom_left_stack.set_halign(Align::Start);
    bottom_left_stack.set_valign(Align::End);
    bottom_left_stack.set_margin_start(16);
    bottom_left_stack.set_margin_bottom(16);
    bottom_left_stack.add_named(&zoom_control, Some(STATIC_PAGE));
    bottom_left_stack.set_visible_child_name(STATIC_PAGE);
    canvas_with_toolbar.add_overlay(&bottom_left_stack);

    let motion_control = GtkBox::new(Orientation::Horizontal, 0);
    motion_control.add_css_class("editor-floating-motion");
    motion_control.set_halign(Align::Center);
    motion_control.set_valign(Align::End);
    motion_control.set_margin_bottom(16);
    motion_control.append(motion_btn);
    canvas_with_toolbar.add_overlay(&motion_control);

    let history_control = GtkBox::new(Orientation::Horizontal, 0);
    history_control.add_css_class("editor-floating-history");
    history_control.set_halign(Align::End);
    history_control.set_valign(Align::End);
    history_control.set_margin_end(16);
    history_control.set_margin_bottom(16);
    history_control.append(history_group);
    canvas_with_toolbar.add_overlay(&history_control);
    canvas_with_toolbar.add_overlay(zoom_popup);

    // Full-width canvas chrome + top inspector band move the window; tools/canvas stay interactive.
    install_window_drag(&top_chrome, window);
    install_top_bar_window_drag(root_overlay, window);
    install_edge_resize(root_overlay, window);

    WindowChrome {
        mode_stack,
        bottom_left_stack,
        motion_control,
        history_control,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_chrome_hosts_toolbar_and_installs_drag_resize() {
        let source = include_str!("chrome.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("canvas_with_toolbar.add_overlay(&top_chrome);")
                && production.contains("install_window_drag(&top_chrome, window);")
                && production.contains("install_top_bar_window_drag(root_overlay, window);")
                && production.contains("install_edge_resize(root_overlay, window);")
                && production.contains("editor-floating-zoom")
                && production.contains("editor-floating-history")
                && production.contains("toolbar.set_halign(Align::Center);")
                && production.contains("mode_stack.set_margin_top(8);")
                && !production.contains("install_window_drag(&toolbar"),
            "chrome must host the centered toolbar with an 8px top gap and install drag/resize without toolbar-only drag"
        );
    }

    #[test]
    fn motion_control_is_bottom_center_and_static_replaces_annotate_toolbar() {
        let source = include_str!("chrome.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("editor-floating-motion")
                && production.contains("motion_control.set_halign(Align::Center);")
                && production.contains("motion_control.set_valign(Align::End);")
                && production.contains("mode_stack.add_named(toolbar, Some(STATIC_PAGE));")
                && production.contains("mode_stack.add_named(static_toolbar, Some(MOTION_PAGE));")
                && production
                    .contains("bottom_left_stack.add_named(&zoom_control, Some(STATIC_PAGE));")
                && !production.contains("play_bar"),
            "Motion button sits bottom-center; Static occupies the top-center toolbar slot"
        );
    }
}
