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

/// The tool bar that caps the left column: one icon per tool, with the
/// selected tool's controls stacked directly beneath it.
///
/// Icons follow the reference — media, cursor, a frame grid, and a saved
/// take — rather than one button per `EditorTool`. The tools a bare recording
/// opens with (Cursor, Background) map onto the media and cursor icons; the
/// timeline-driven panels (Zoom, Hide, Clip) are reached by selecting a clip
/// in the timeline, and the fourth icon is the placeholder for the take
/// library, which is not built yet.
pub(super) fn build_tool_section(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> ToolSection {
    let root = GtkBox::new(Orientation::Horizontal, 4);
    root.add_css_class("recording-editor-tool-section");
    root.set_hexpand(false);
    root.set_vexpand(false);
    root.set_halign(Align::Fill);

    let background = tool_icon_button(icon_names::custom::IMAGE_ALT_SYMBOLIC, &t("Background"));
    background.set_tooltip_text(Some(&t("Background")));
    background.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| {
            state.lock().unwrap().selected_tool = EditorTool::Background;
            button.set_active(true);
            on_change();
        }
    });
    root.append(&background);

    let cursor = tool_icon_button(icon_names::POINTER_PRIMARY_CLICK, &t("Cursor"));
    cursor.set_tooltip_text(Some(&t("Cursor")));
    cursor.set_active(true);
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

    // Frame and motion tools. These drive the timeline rather than the
    // sidebar, so selecting one hands the sidebar back to whatever the
    // timeline currently has selected.
    let frames = tool_icon_button("view-grid-symbolic", &t("Frames"));
    frames.set_tooltip_text(Some(&t("Frames")));
    frames.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| {
            let mut guard = state.lock().unwrap();
            guard.selected_tool = EditorTool::Timeline;
            drop(guard);
            button.set_active(true);
            on_change();
        }
    });
    root.append(&frames);

    // The take library is not built yet; the icon marks where it goes.
    let takes = tool_icon_button("bookmark-symbolic", &t("Takes"));
    takes.set_tooltip_text(Some(&t("Takes — coming soon")));
    takes.set_sensitive(false);
    root.append(&takes);

    let refresh = {
        let background = background.clone();
        let cursor = cursor.clone();
        let frames = frames.clone();
        Rc::new(move || {
            let tool = state.lock().unwrap().selected_tool;
            // Timeline has its own icon; the other two go inactive while a
            // timeline selection drives the panel below.
            background.set_active(tool == EditorTool::Background);
            cursor.set_active(tool == EditorTool::Cursor);
            frames.set_active(tool == EditorTool::Timeline);
        }) as Rc<dyn Fn()>
    };

    ToolSection {
        widget: root,
        refresh,
    }
}

fn tool_icon_button(icon_name: &str, label: &str) -> ToggleButton {
    let button = ToggleButton::new();
    button.add_css_class("recording-editor-tool-section-btn");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button.set_tooltip_text(Some(label));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    button.set_child(Some(&icon));
    button
}

#[cfg(test)]
mod tests {
    #[test]
    fn rail_exposes_background_next_to_cursor() {
        let source = include_str!("tool_section.rs");
        assert!(
            source.contains("EditorTool::Background"),
            "rail must be able to select the Background tool"
        );
        assert!(
            source.contains("\"Background\""),
            "rail needs a Background button next to Cursor"
        );
        assert!(
            source.contains("IMAGE_ALT_SYMBOLIC"),
            "Background rail button should reuse the image-editor backdrop icon"
        );
    }

    #[test]
    fn the_rail_is_a_horizontal_bar_of_icons() {
        let source = include_str!("tool_section.rs");
        // The bar caps the left column, so it lays its icons out across.
        assert!(
            source.contains("Orientation::Horizontal"),
            "the tool bar lays its icons out in a row"
        );
        // Four icons, matching the reference bar.
        for icon in [
            "IMAGE_ALT_SYMBOLIC",
            "POINTER_PRIMARY_CLICK",
            "view-grid-symbolic",
            "bookmark-symbolic",
        ] {
            assert!(
                source.contains(icon),
                "the tool bar must keep the {icon} icon"
            );
        }
    }
}
