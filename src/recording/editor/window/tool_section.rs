use crate::capture::editor::window::icon_names;
use crate::recording::editor::model::{EditorTool, VideoEditState};
use gtk4::{prelude::*, Align, Box as GtkBox, Image, Orientation, ToggleButton};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::t;

pub(super) struct ToolSection {
    pub widget: GtkBox,
    pub refresh: Rc<dyn Fn()>,
}

pub(super) fn build_tool_section(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> ToolSection {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.add_css_class("recording-editor-tool-section");
    root.set_hexpand(false);
    root.set_vexpand(true);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Fill);

    let cursor = ToggleButton::new();
    cursor.add_css_class("recording-editor-tool-section-btn");
    cursor.set_has_frame(false);
    cursor.set_tooltip_text(Some(&t("Cursor")));
    cursor.set_halign(Align::Center);
    let icon = Image::from_icon_name(icon_names::POINTER_PRIMARY_CLICK);
    icon.set_pixel_size(18);
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    cursor.set_child(Some(&icon));
    cursor.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| {
            state.lock().unwrap().selected_tool = EditorTool::Cursor;
            button.set_active(true);
            on_change();
        }
    });
    root.append(&cursor);

    let refresh = {
        let cursor = cursor.clone();
        Rc::new(move || {
            let tool = state.lock().unwrap().selected_tool;
            cursor.set_active(tool == EditorTool::Cursor);
        }) as Rc<dyn Fn()>
    };

    ToolSection {
        widget: root,
        refresh,
    }
}
