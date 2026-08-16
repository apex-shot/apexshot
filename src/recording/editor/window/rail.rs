use super::media_library;
use crate::recording::editor::model::VideoEditState;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, Image, Orientation, Revealer};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RailTool {
    Media,
    Zoom,
}

pub(super) struct ToolChrome {
    pub rail: GtkBox,
    pub library: Revealer,
}

pub(super) fn build_tool_chrome(
    state: Option<Arc<Mutex<VideoEditState>>>,
    estimate_label: Option<gtk4::Label>,
) -> ToolChrome {
    let library = media_library::build_media_library(state.clone());
    let rail = build_tool_rail(state, estimate_label, library.clone());
    ToolChrome { rail, library }
}

fn build_tool_rail(
    state: Option<Arc<Mutex<VideoEditState>>>,
    estimate_label: Option<gtk4::Label>,
    library: Revealer,
) -> GtkBox {
    let rail = GtkBox::new(Orientation::Vertical, 10);
    rail.add_css_class("recording-editor-tool-rail");
    rail.add_css_class("editor-right-inspector");
    rail.add_css_class("recording-editor-inspector");
    rail.set_hexpand(false);
    rail.set_vexpand(true);
    rail.set_halign(Align::Start);

    let selected = Rc::new(Cell::new(RailTool::Media));

    let add = icon_rail_button("list-add-symbolic", "Add media");
    add.add_css_class("recording-editor-tool-rail-add");
    let _ = (state, estimate_label);

    let media = icon_rail_button("camera-video-symbolic", "Media");
    let zoom = icon_rail_button("zoom-in-symbolic", "Zoom");
    media.add_css_class("recording-editor-tool-rail-active");

    {
        let selected = selected.clone();
        let media_btn = media.clone();
        let zoom_btn = zoom.clone();
        let library = library.clone();
        media.connect_clicked(move |_| {
            selected.set(RailTool::Media);
            set_active(&media_btn, &zoom_btn, RailTool::Media);
            library.set_reveal_child(true);
        });
    }
    {
        let selected = selected.clone();
        let media = media.clone();
        let zoom_btn = zoom.clone();
        let library = library.clone();
        zoom.connect_clicked(move |_| {
            selected.set(RailTool::Zoom);
            set_active(&media, &zoom_btn, RailTool::Zoom);
            library.set_reveal_child(false);
        });
    }

    add.connect_clicked({
        let library = library.clone();
        let selected = selected.clone();
        let media_btn = media.clone();
        let zoom_btn = zoom.clone();
        move |_| {
            selected.set(RailTool::Media);
            set_active(&media_btn, &zoom_btn, RailTool::Media);
            library.set_reveal_child(true);
        }
    });

    let tools = GtkBox::new(Orientation::Vertical, 6);
    tools.add_css_class("recording-editor-tool-rail-group");
    tools.append(&add);
    tools.append(&media);
    tools.append(&zoom);

    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    rail.append(&tools);
    rail.append(&spacer);
    rail
}

fn set_active(media: &Button, zoom: &Button, tool: RailTool) {
    media.remove_css_class("recording-editor-tool-rail-active");
    zoom.remove_css_class("recording-editor-tool-rail-active");
    match tool {
        RailTool::Media => media.add_css_class("recording-editor-tool-rail-active"),
        RailTool::Zoom => zoom.add_css_class("recording-editor-tool-rail-active"),
    }
}

fn icon_rail_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-tool-rail-button");
    button.set_tooltip_text(Some(tooltip));
    button.set_halign(Align::Center);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    button.set_child(Some(&icon));
    button
}
