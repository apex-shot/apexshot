//! The Motion mode page: the checkerboard preview, the timeline dock, and
//! the tool notch. Entering Motion swaps the workspace and top toolbar the
//! way the Linux editor swaps its stacks; the notch tabs switch the dock's
//! pages.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Image, Orientation, Overlay};

use super::canvas::draw_canvas_checkerboard;
use super::inspector::Inspector;
use super::timeline;
use super::ui_support::px;
use crate::capture::editor::window::icon_names::{custom, shipped};
use crate::i18n::t;

pub(super) struct MotionMode {
    /// The page the workspace stack shows while Motion is open.
    pub page: GtkBox,
    /// The top toolbar's Motion page: one labelled Static button.
    pub toolbar: GtkBox,
    pub static_button: Button,
    pub timeline: Rc<timeline::Timeline>,
    inspector: Rc<Inspector>,
    tabs: Rc<std::cell::RefCell<Vec<(Button, &'static str)>>>,
}

impl MotionMode {
    /// Select one notch tab: highlight it and show its dock page. Entry
    /// always lands on Composition, like the Linux editor applies its mode.
    pub(super) fn set_active_tab(&self, page: &'static str) {
        for (button, button_page) in self.tabs.borrow().iter() {
            if *button_page == page {
                button.add_css_class("active-tool");
            } else {
                button.remove_css_class("active-tool");
            }
        }
        self.inspector.show_motion_page(page);
    }
}

pub(super) fn build(scale: f64, dark: bool, inspector: Rc<Inspector>) -> MotionMode {
    // Preview: the checkerboard backdrop, with the motion drawing area over
    // it once a backend exists. Both fill the space above the dock.
    let backdrop = DrawingArea::new();
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.set_can_target(false);
    backdrop.set_draw_func(move |_, context, width, height| {
        draw_canvas_checkerboard(context, f64::from(width), f64::from(height), scale, dark);
    });
    let preview = DrawingArea::new();
    preview.set_hexpand(true);
    preview.set_vexpand(true);
    preview.set_can_target(false);
    let preview_shell = Overlay::new();
    preview_shell.set_hexpand(true);
    preview_shell.set_vexpand(true);
    preview_shell.set_child(Some(&backdrop));
    preview_shell.add_overlay(&preview);

    let timeline = timeline::build(scale, dark);

    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.set_focusable(true);
    page.append(&preview_shell);
    page.append(&timeline.dock);

    // The mode toolbar keeps the annotate toolbar's pill shape and top gap;
    // Static rides back to the editor the button was clicked from.
    let toolbar = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
    toolbar.add_css_class("image-editor-toolbar");
    toolbar.set_halign(Align::Center);
    toolbar.set_valign(Align::Start);
    toolbar.set_margin_top(px(8.0, scale));
    let static_button = Button::with_label(&t("Static"));
    static_button.add_css_class("image-editor-tool-button");
    static_button.add_css_class("image-editor-mode-button");
    static_button.set_focusable(false);
    static_button.set_tooltip_text(Some(&t("Switch to Static")));
    toolbar.append(&static_button);

    // The tool notch hugs the preview's right edge, midway to the timeline,
    // exactly the Linux placement. One Motion tool per notch button.
    let notch = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    notch.add_css_class("editor-motion-tool-notch");
    notch.set_halign(Align::End);
    notch.set_valign(Align::Center);
    let tabs: Rc<std::cell::RefCell<Vec<(Button, &'static str)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let tab_specs: [(&'static str, &str, &str); 4] = [
        ("motion", custom::SPLINE_SYMBOLIC, "Motion"),
        ("motion-text", custom::TYPE_SYMBOLIC, "Text"),
        (
            "motion-appearance",
            custom::IMAGE_ALT_SYMBOLIC,
            "Appearance",
        ),
        ("motion-watermark", shipped::IMAGE_REGULAR, "Watermark"),
    ];
    for (name, icon_name, tooltip) in tab_specs {
        let button = Button::new();
        button.set_has_frame(false);
        button.set_tooltip_text(Some(&t(tooltip)));
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(px(20.0, scale));
        button.set_child(Some(&icon));
        button.add_css_class("image-editor-notch-button");
        button.set_focusable(false);
        notch.append(&button);
        tabs.borrow_mut().push((button.clone(), name));
        let inspector_for_tab = inspector.clone();
        let tabs_for_click = tabs.clone();
        let page_name = name;
        button.connect_clicked(move |_| {
            inspector_for_tab.show_motion_page(page_name);
            for (tab_button, tab_page) in tabs_for_click.borrow().iter() {
                if *tab_page == page_name {
                    tab_button.add_css_class("active-tool");
                } else {
                    tab_button.remove_css_class("active-tool");
                }
            }
        });
    }
    preview_shell.add_overlay(&notch);

    MotionMode {
        page,
        toolbar,
        static_button,
        timeline: Rc::new(timeline),
        inspector,
        tabs,
    }
}
