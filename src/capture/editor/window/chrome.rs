//! Concrete editor window chrome: top drag strip, floating controls, edge resize (PR 10.14).
//!
//! Owns top-chrome construction over the canvas, floating zoom/history overlays,
//! and window drag/resize controller installation. Setup still owns workspace
//! assembly and session bootstrap.

use gtk4::{prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Orientation, Overlay};

use super::super::ui_support::{
    install_edge_resize, install_top_bar_window_drag, install_window_drag, EDITOR_TOP_CHROME_HEIGHT,
};

pub(super) struct WindowChromeInputs<'a> {
    pub canvas_with_toolbar: &'a Overlay,
    pub root_overlay: &'a Overlay,
    pub window: &'a ApplicationWindow,
    pub toolbar: &'a GtkBox,
    pub zoom_minus_btn: &'a Button,
    pub zoom_button: &'a Button,
    pub zoom_plus_btn: &'a Button,
    pub history_group: &'a GtkBox,
    pub zoom_popup: &'a GtkBox,
}

/// Build top chrome + floating controls and install window drag/resize handlers.
pub(super) fn install_window_chrome(input: WindowChromeInputs<'_>) {
    let WindowChromeInputs {
        canvas_with_toolbar,
        root_overlay,
        window,
        toolbar,
        zoom_minus_btn,
        zoom_button,
        zoom_plus_btn,
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

    toolbar.set_halign(Align::Center);
    toolbar.set_valign(Align::Center);
    toolbar.set_hexpand(false);
    top_chrome.append(&top_chrome_left);
    top_chrome.append(toolbar);
    top_chrome.append(&top_chrome_right);
    canvas_with_toolbar.add_overlay(&top_chrome);

    let zoom_control = GtkBox::new(Orientation::Horizontal, 0);
    zoom_control.add_css_class("editor-floating-zoom");
    zoom_control.set_halign(Align::Start);
    zoom_control.set_valign(Align::End);
    zoom_control.set_margin_start(16);
    zoom_control.set_margin_bottom(16);
    zoom_control.append(zoom_minus_btn);
    zoom_control.append(zoom_button);
    zoom_control.append(zoom_plus_btn);
    canvas_with_toolbar.add_overlay(&zoom_control);

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
                && !production.contains("install_window_drag(&toolbar"),
            "chrome must host toolbar on top strip and install drag/resize without toolbar-only drag"
        );
    }
}
