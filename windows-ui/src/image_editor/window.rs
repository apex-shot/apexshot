//! The Windows image-editor window.
//!
//! The frame follows the Windows Settings and video-editor shells: a custom
//! 32px caption bar, one rounded outline, the Inter face, and DPI-scaled
//! metrics. Inside it, the floating tool bar and the inspector frame sit around
//! the canvas that owns the screenshot.

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::settings::ui_support::{
    caption_button, CaptionGlyph, CAPTION_BUTTON_WIDTH, TITLE_BAR_HEIGHT,
};
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
use super::ui_support::{self, px, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};

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

    let document_title = super::document_title(initial);
    let title_label = gtk4::Label::new(Some(&document_title));
    title_label.set_tooltip_text(Some(&document_title));
    let title_bar = build_title_bar(&window, &title_label, scale);
    root.append(&title_bar);

    let on_loaded: Rc<dyn Fn(&Path)> = {
        let title_label = title_label.clone();
        Rc::new(move |path| {
            let title = super::document_title(Some(path));
            title_label.set_text(&title);
            title_label.set_tooltip_text(Some(&title));
        })
    };
    let canvas = canvas::build(scale, dark, on_loaded);

    let inspector = Rc::new(inspector::build(scale));
    let toolbar = toolbar::build(scale, Tool::Select, {
        let inspector = inspector.clone();
        Rc::new(move |tool| inspector.show_tool(tool))
    });
    canvas.root.add_overlay(&toolbar.root);

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

fn build_title_bar(window: &ApplicationWindow, title: &gtk4::Label, scale: f64) -> GtkBox {
    let bar = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    bar.add_css_class("settings-window-controls");
    bar.add_css_class("image-editor-titlebar");
    bar.set_size_request(-1, (TITLE_BAR_HEIGHT * scale).round() as i32);

    // Balance the title over the window the way the video editor does: reserve
    // the caption controls on both sides so a document name stays centred.
    let controls_width = (CAPTION_BUTTON_WIDTH * scale).round() as i32 * 3;
    let left_handle = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    left_handle.set_size_request(controls_width, -1);
    bar.append(&left_handle);

    title.add_css_class("image-editor-document-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_valign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    title.set_max_width_chars(72);
    bar.append(title);

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

    let actions = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    actions.set_halign(Align::End);
    actions.set_valign(Align::Fill);
    actions.append(&minimize);
    actions.append(&maximize);
    actions.append(&close);

    let right_balance = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    right_balance.set_size_request(controls_width, -1);
    right_balance.append(&actions);
    bar.append(&right_balance);

    windowing::install_window_drag(&left_handle, window);
    windowing::install_window_drag(title, window);

    bar
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

#[cfg(test)]
mod tests {
    use super::super::document_title;
    use std::path::Path;

    #[test]
    fn the_title_bar_names_the_open_file() {
        assert_eq!(
            document_title(Some(Path::new(r"C:\shots\Capture 2026-10-31.png"))),
            "Capture 2026-10-31.png"
        );
    }

    #[test]
    fn an_empty_session_keeps_the_untitled_title() {
        assert_eq!(document_title(None), "Untitled screenshot");
    }
}
