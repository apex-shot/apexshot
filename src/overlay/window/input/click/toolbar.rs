//! Toolbar and recording-panel click state transitions.

use super::ClickEffect;
use crate::capture_overlay::RecordingType;
use crate::overlay::api::OverlaySelection;
use crate::overlay::geometry::{current_selection_rect, is_inside_selection};
use crate::overlay::hit_testing::toolbar_hit_at;
use crate::overlay::icons::{
    ToolbarIcon, TOOLBAR_AREA_INDEX, TOOLBAR_FULLSCREEN_INDEX, TOOLBAR_ICONS,
    TOOLBAR_RECORDING_INDEX, TOOLBAR_SCROLL_INDEX,
};
use crate::overlay::layout::{
    ToolbarHit, DEFAULT_SELECTION_HEIGHT, DEFAULT_SELECTION_WIDTH, MIN_SELECTION_HEIGHT,
    MIN_SELECTION_WIDTH,
};
use crate::overlay::recording::hit_testing::recording_tile_at;
use crate::overlay::recording::layout::RecordPanelTile;
use crate::overlay::recording::state::OverlayIntent;
use crate::overlay::state::SelectorState;
use crate::overlay::window::result::recording_request_from_state;

pub(super) fn handle_toolbar_click(
    st: &mut SelectorState,
    n_press: i32,
    x: f64,
    y: f64,
    screen_width: i32,
    screen_height: i32,
) -> ClickEffect {
    let rect = current_selection_rect(st);
    let recording_panel_open = st.recording.panel_open;
    let record_hit = recording_panel_open
        .then(|| {
            recording_tile_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            )
        })
        .flatten();
    let hit = if recording_panel_open {
        None
    } else {
        toolbar_hit_at(
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            screen_width as f64,
            screen_height as f64,
            x,
            y,
        )
    };
    // Capture-menu Area already committed the capture intent.  Its C++
    // counterpart keeps frame/crop controls but disables the legacy tool rail.
    let hit = if st.capture_menu_area_mode && matches!(hit, Some(ToolbarHit::Tool(_))) {
        None
    } else {
        hit
    };
    let clicked = match hit {
        Some(ToolbarHit::Tool(index)) => Some(TOOLBAR_ICONS[index]),
        _ => None,
    };

    match clicked {
        Some(ToolbarIcon::Fullscreen) => {
            st.active_tool_index = TOOLBAR_FULLSCREEN_INDEX;
            st.intent = OverlayIntent::Area;
            st.start_x = 0.0;
            st.start_y = 0.0;
            st.current_x = screen_width as f64;
            st.current_y = screen_height as f64;
            st.completed = true;
            st.is_dragging = false;
            st.fullscreen_mode = true;
            ClickEffect::Redraw
        }
        Some(ToolbarIcon::Area) => {
            st.active_tool_index = TOOLBAR_AREA_INDEX;
            st.intent = OverlayIntent::Area;
            let screen_w = screen_width as f64;
            let screen_h = screen_height as f64;
            let width = DEFAULT_SELECTION_WIDTH
                .min(screen_w)
                .max(MIN_SELECTION_WIDTH.min(screen_w));
            let height = DEFAULT_SELECTION_HEIGHT
                .min(screen_h)
                .max(MIN_SELECTION_HEIGHT.min(screen_h));
            st.start_x = ((screen_w - width) / 2.0).max(0.0);
            st.start_y = ((screen_h - height) / 2.0).max(0.0);
            st.current_x = st.start_x + width;
            st.current_y = st.start_y + height;
            st.completed = true;
            st.is_dragging = false;
            st.fullscreen_mode = false;
            st.recording.panel_open = false;
            ClickEffect::Redraw
        }
        Some(ToolbarIcon::Recording) => {
            st.active_tool_index = TOOLBAR_RECORDING_INDEX;
            st.recording.panel_open = true;
            st.intent = OverlayIntent::Record;
            clear_toolbar_hover(st);
            ClickEffect::Redraw
        }
        Some(ToolbarIcon::Timer) => {
            if !st.timer_delay_active {
                st.timer_delay_active = true;
                if st.capture_delay_seconds <= 0 {
                    st.capture_delay_seconds = 5;
                }
            } else {
                st.capture_delay_seconds = match st.capture_delay_seconds {
                    3 => 5,
                    5 => 10,
                    _ => 0,
                };
            }
            st.timer_delay_active = st.capture_delay_seconds > 0;
            st.hover_tool_index = None;
            ClickEffect::Redraw
        }
        Some(ToolbarIcon::Scroll) => {
            st.capture_crop_menu_open = false;
            st.scroll_popup_open = true;
            st.active_tool_index = TOOLBAR_SCROLL_INDEX;
            st.intent = OverlayIntent::Area;
            st.hover_tool_index = None;
            ClickEffect::Redraw
        }
        Some(ToolbarIcon::Ocr) => {
            st.active_tool_index = crate::overlay::icons::TOOLBAR_OCR_INDEX;
            st.intent = OverlayIntent::Ocr;
            st.hover_tool_index = None;
            ClickEffect::Redraw
        }
        _ => handle_panel_or_selection_click(
            st,
            n_press,
            x,
            y,
            hit,
            record_hit,
            recording_panel_open,
        ),
    }
}

fn handle_panel_or_selection_click(
    st: &mut SelectorState,
    n_press: i32,
    x: f64,
    y: f64,
    hit: Option<ToolbarHit>,
    record_hit: Option<RecordPanelTile>,
    recording_panel_open: bool,
) -> ClickEffect {
    if !recording_panel_open && hit == Some(ToolbarHit::CropPanel) {
        st.capture_crop_menu_open = !st.capture_crop_menu_open;
        st.hovered_capture_crop_menu_item = -1;
        st.hover_tool_index = None;
        return ClickEffect::Redraw;
    }

    if let Some(tile) = record_hit {
        match tile {
            RecordPanelTile::Crop => {
                st.recording.crop_menu_open = !st.recording.crop_menu_open;
                st.recording.hovered_crop_menu_item = -1;
                st.recording.settings_menu_open = false;
                st.recording.settings_dropdown_open = None;
                st.recording.hovered_settings_dropdown_item = -1;
                st.recording.mic_volume_popup_open = false;
                st.recording.speaker_volume_popup_open = false;
                st.hover_tool_index = None;
            }
            RecordPanelTile::Controls => {
                st.recording.settings_menu_open = !st.recording.settings_menu_open;
                st.recording.hovered_settings_item = -1;
                st.recording.settings_dropdown_open = None;
                st.recording.hovered_settings_dropdown_item = -1;
                st.recording.crop_menu_open = false;
                st.recording.mic_volume_popup_open = false;
                st.recording.speaker_volume_popup_open = false;
                st.recording.hover_record_tile = None;
                st.hover_tool_index = None;
            }
            RecordPanelTile::Mic => {
                st.recording.mic_toggle = !st.recording.mic_toggle;
                st.recording.mic_volume_popup_open = false;
            }
            RecordPanelTile::Speaker => {
                st.recording.speaker_toggle = !st.recording.speaker_toggle;
                st.recording.speaker_volume_popup_open = false;
            }
            RecordPanelTile::Size => {}
            RecordPanelTile::RecordVideo | RecordPanelTile::RecordGif => {
                let record_type = if matches!(tile, RecordPanelTile::RecordGif) {
                    RecordingType::Gif
                } else {
                    RecordingType::Video
                };
                if st.recording.selected_record_type == Some(record_type) {
                    let request = recording_request_from_state(st, record_type);
                    return ClickEffect::SendRecording(OverlaySelection::Recording(request));
                }
                st.recording.selected_record_type = Some(record_type);
                st.recording.crop_menu_open = false;
                st.recording.settings_menu_open = false;
                st.recording.settings_dropdown_open = None;
                st.recording.hovered_settings_dropdown_item = -1;
                st.recording.mic_volume_popup_open = false;
                st.recording.speaker_volume_popup_open = false;
                st.recording.hover_record_tile = None;
                st.hover_tool_index = None;
            }
        }
        return ClickEffect::Redraw;
    }

    if n_press == 2 && st.completed && is_inside_selection(x, y, current_selection_rect(st)) {
        ClickEffect::SendSelection
    } else {
        ClickEffect::None
    }
}

fn clear_toolbar_hover(st: &mut SelectorState) {
    st.hover_tool_index = None;
    st.hover_size_panel = false;
    st.hover_crop_panel = false;
}

#[cfg(test)]
mod tests {
    #[test]
    fn toolbar_owner_covers_tools_recording_tiles_and_double_click() {
        let source = include_str!("toolbar.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production toolbar owner");
        for tool in ["Fullscreen", "Area", "Recording", "Timer", "Scroll", "Ocr"] {
            assert!(
                production.contains(&format!("ToolbarIcon::{tool}")),
                "missing toolbar tool {tool}"
            );
        }
        assert!(
            production.contains("RecordPanelTile::Crop")
                && production.contains("RecordPanelTile::RecordVideo")
                && production.contains("RecordPanelTile::RecordGif"),
            "toolbar owner must cover recording panel tiles"
        );
        assert!(
            production.contains("n_press == 2")
                && production.contains("ClickEffect::SendSelection")
                && production.contains("ClickEffect::SendRecording"),
            "toolbar owner must return delivery effects"
        );
        assert!(
            !production.contains("result_tx")
                && !production.contains("queue_draw")
                && !production.contains("window.close"),
            "toolbar state owner must not perform channel or GTK effects"
        );
    }
}
