use crate::recording::editor::cursor_sprite;
use crate::recording::editor::model::{EditorTool, VideoEditState};
use gtk4::{prelude::*, Align, Box as GtkBox, DrawingArea, Orientation, ToggleButton};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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
    cursor.set_tooltip_text(Some("Cursor"));
    cursor.set_halign(Align::Center);
    let glyph = DrawingArea::new();
    glyph.set_content_width(24);
    glyph.set_content_height(24);
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    glyph.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            let theme = state.lock().unwrap().cursor.theme;
            cursor_sprite::draw_centered(cr, width as f64, height as f64, theme);
        }
    });
    cursor.set_child(Some(&glyph));
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
        let glyph = glyph.clone();
        Rc::new(move || {
            let tool = state.lock().unwrap().selected_tool;
            cursor.set_active(tool == EditorTool::Cursor);
            glyph.queue_draw();
        }) as Rc<dyn Fn()>
    };

    ToolSection {
        widget: root,
        refresh,
    }
}
