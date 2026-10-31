//! The Windows image-editor window.
//!
//! The frame follows the Linux editor: no caption bar, the checkerboard canvas
//! fills the window, and the floating tool bar rides in a transparent top
//! chrome strip that doubles as the drag handle. The caption controls sit at
//! the top of the inspector, the same place the Linux editor puts its window
//! controls.

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::settings::ui_support::{caption_button, CaptionGlyph};
use crate::settings::windowing;
use gtk4::gdk;
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{Align, Application, ApplicationWindow, Box as GtkBox};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Once;

use super::canvas;
use super::inspector;
use super::toolbar::{self, Tool};
use super::ui_support::{self, px, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, TOP_CHROME_HEIGHT};

pub(super) fn build_window(app: &Application, initial: Option<&Path>) {
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    ui_support::install_css();

    let scale = crate::platform::ui_scale();
    let (default_width, default_height) = ui_support::default_window_size(scale);
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Editor"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(default_width)
        .default_height(default_height)
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");
    window.add_css_class("image-editor-window");
    window.set_size_request(px(MIN_WINDOW_WIDTH, scale), px(MIN_WINDOW_HEIGHT, scale));

    let dark = windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(gtk4::Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("image-editor-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }
    // Keep the canvas inside the shell's rounded corners.
    root.set_overflow(gtk4::Overflow::Hidden);

    let canvas = canvas::build(scale, dark);

    let inspector = Rc::new(inspector::build(scale));
    let sub_bars = Rc::new(super::sub_bar::build(scale, {
        let canvas = canvas.clone();
        Rc::new(move |docked| canvas.set_docked(docked))
    }));
    let toolbar = toolbar::build(scale, Tool::Select, {
        let inspector = inspector.clone();
        let sub_bars = sub_bars.clone();
        Rc::new(move |tool| {
            inspector.show_tool(tool);
            sub_bars.show_tool(tool);
        })
    });
    canvas.root.add_overlay(&sub_bars.root);

    // Transparent strip over the canvas' top edge: it centres the tool bar and
    // is the strip the window drags from, like the Linux top chrome.
    let top_chrome = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_chrome.add_css_class("image-editor-top-chrome");
    top_chrome.set_halign(Align::Fill);
    top_chrome.set_valign(Align::Start);
    top_chrome.set_hexpand(true);
    top_chrome.set_size_request(-1, px(TOP_CHROME_HEIGHT, scale));
    let top_left = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_left.set_hexpand(true);
    let top_right = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_right.set_hexpand(true);
    top_chrome.append(&top_left);
    top_chrome.append(&toolbar.root);
    top_chrome.append(&top_right);
    canvas.root.add_overlay(&top_chrome);
    canvas.root.add_overlay(&toolbar.history);
    windowing::install_window_drag(&top_chrome, &window);

    let utility = build_utility_strip(&window, scale);
    inspector.root.prepend(&utility);

    let workspace = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    workspace.add_css_class("image-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.append(&canvas.root);
    workspace.append(&inspector.root);
    root.append(&workspace);

    window.set_child(Some(&root));

    canvas.open_button.connect_clicked({
        let canvas = canvas.clone();
        let window = window.clone();
        move |_| canvas.choose_image(&window)
    });

    install_keys(&window, &canvas);

    window.present();

    if let Some(path) = initial {
        canvas.load(path);
    }
}

/// The caption controls at the top of the inspector: an empty drag area on the
/// left, then minimize, maximize and close hugging the window's top-right
/// corner, where the Linux utility strip keeps its window buttons.
fn build_utility_strip(window: &ApplicationWindow, scale: f64) -> GtkBox {
    let strip = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    strip.add_css_class("image-editor-utility-controls");
    strip.set_halign(Align::Fill);

    let drag_area = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    drag_area.set_hexpand(true);
    strip.append(&drag_area);
    windowing::install_window_drag(&drag_area, window);

    let (minimize, _) = caption_button(
        Rc::new(Cell::new(CaptionGlyph::Minimize)),
        &t("Minimize"),
        scale,
    );
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());

    let maximize_state = Rc::new(Cell::new(CaptionGlyph::Maximize));
    let (maximize, maximize_glyph) = caption_button(maximize_state.clone(), &t("Maximize"), scale);
    let maximize_window = window.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
    });
    window.connect_maximized_notify({
        let state = maximize_state;
        move |window| {
            state.set(if window.is_maximized() {
                CaptionGlyph::Restore
            } else {
                CaptionGlyph::Maximize
            });
            maximize_glyph.queue_draw();
        }
    });

    let (close, _) = caption_button(Rc::new(Cell::new(CaptionGlyph::Close)), &t("Close"), scale);
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());

    strip.append(&minimize);
    strip.append(&maximize);
    strip.append(&close);
    strip
}

fn install_keys(window: &ApplicationWindow, canvas: &Rc<canvas::Canvas>) {
    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed({
        let canvas = canvas.clone();
        let window = window.clone();
        move |_, key, _, modifiers| {
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            match key {
                gdk::Key::Escape => {
                    window.close();
                    Propagation::Stop
                }
                gdk::Key::_0 | gdk::Key::KP_0 if ctrl => {
                    canvas.fit();
                    Propagation::Stop
                }
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add if ctrl => {
                    canvas.zoom_in();
                    Propagation::Stop
                }
                gdk::Key::minus | gdk::Key::underscore | gdk::Key::KP_Subtract if ctrl => {
                    canvas.zoom_out();
                    Propagation::Stop
                }
                gdk::Key::o | gdk::Key::O if ctrl => {
                    canvas.choose_image(&window);
                    Propagation::Stop
                }
                gdk::Key::w | gdk::Key::W if ctrl => {
                    window.close();
                    Propagation::Stop
                }
                _ => Propagation::Proceed,
            }
        }
    });
    window.add_controller(keys);
}
