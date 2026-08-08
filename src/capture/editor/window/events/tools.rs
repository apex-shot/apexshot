//! Tool-mode button activation for the editor toolbar.
//!
//! Owns Select, Crop, Background, Pen, Arrow, Line, Box, Circle, Text, Number,
//! Obfuscate, Focus, and Highlighter click handlers. Toggle policies differ by
//! tool and must stay distinct (crop init, number/highlighter toggle-off,
//! obfuscate effect rebuild recovery, pen cursor).
//!
//! Crop apply/reset buttons stay in `crop.rs`; canvas drag/click stay separate.

use gtk4::{prelude::*, ApplicationWindow, Button, DrawingArea};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    state::EditorState,
    types::{tool_button_index, Tool},
    ui_support::{set_active_tool_button, set_crop_apply_button_state},
};

use super::super::cursor::{set_window_cursor_name, update_pen_cursor};

pub(super) struct ToolModeButtons<'a> {
    pub tool_buttons: &'a [Button],
    pub select: &'a Button,
    pub crop: &'a Button,
    pub background: &'a Button,
    pub pen: &'a Button,
    pub arrow: &'a Button,
    pub line: &'a Button,
    pub boxed: &'a Button,
    pub circle: &'a Button,
    pub text: &'a Button,
    pub number: &'a Button,
    pub highlighter: &'a Button,
    pub obfuscate: &'a Button,
    pub focus: &'a Button,
    pub apply_crop: &'a Button,
}

pub(super) fn wire_tool_mode_switches(
    buttons: ToolModeButtons<'_>,
    window: &ApplicationWindow,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    update_toolbar_for_tool: &Rc<dyn Fn(Tool)>,
    update_crop_size_fields: &Rc<dyn Fn()>,
    sync_picker_for_active_tool: &Rc<dyn Fn()>,
    sync_select_inspector: &Rc<dyn Fn()>,
    sync_size_control: &Rc<dyn Fn()>,
    rebuild_effects_async: &Rc<dyn Fn()>,
) {
    let ToolModeButtons {
        tool_buttons,
        select,
        crop,
        background,
        pen,
        arrow,
        line,
        boxed,
        circle,
        text,
        number,
        highlighter,
        obfuscate,
        focus,
        apply_crop,
    } = buttons;

    let state_select = state.clone();
    let drawing_area_select = drawing_area.downgrade();
    let buttons_select = tool_buttons.to_vec();
    let apply_crop_btn_select = apply_crop.clone();
    let update_toolbar_for_tool_select = update_toolbar_for_tool.clone();
    let sync_size_control_select = sync_size_control.clone();
    let sync_select_inspector_select = sync_select_inspector.clone();
    let rebuild_effects_async_select = rebuild_effects_async.clone();
    select.connect_clicked(move |_| {
        set_active_tool_button(&buttons_select, tool_button_index(Tool::Select));
        if state_select
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Select)
        {
            rebuild_effects_async_select();
        }
        update_toolbar_for_tool_select(Tool::Select);
        sync_select_inspector_select();
        sync_size_control_select();
        set_crop_apply_button_state(&apply_crop_btn_select, false, false);
        if let Some(area) = drawing_area_select.upgrade() {
            area.queue_draw();
        }
    });

    let state_crop = state.clone();
    let drawing_area_crop = drawing_area.downgrade();
    let buttons_crop = tool_buttons.to_vec();
    let apply_crop_btn_crop = apply_crop.clone();
    let update_toolbar_for_tool_crop = update_toolbar_for_tool.clone();
    let update_crop_size_fields_crop = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_crop = sync_picker_for_active_tool.clone();
    let sync_size_control_crop = sync_size_control.clone();
    let rebuild_effects_async_crop = rebuild_effects_async.clone();
    crop.connect_clicked(move |_| {
        let (next_tool, has_selection) = {
            let mut state = state_crop.lock().unwrap();
            let rebuild = if state.selected_tool == Tool::Crop {
                let rebuild = state.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, false, rebuild)
            } else {
                let rebuild = state.set_tool_without_rebuild(Tool::Crop);
                state.ensure_crop_selection_initialized();
                (Tool::Crop, state.crop_selection.is_some(), rebuild)
            };
            if rebuild.2 {
                rebuild_effects_async_crop();
            }
            (rebuild.0, rebuild.1)
        };
        set_active_tool_button(&buttons_crop, tool_button_index(next_tool));
        update_toolbar_for_tool_crop(next_tool);
        sync_picker_for_active_tool_crop();
        sync_size_control_crop();
        set_crop_apply_button_state(
            &apply_crop_btn_crop,
            matches!(next_tool, Tool::Crop),
            has_selection,
        );
        update_crop_size_fields_crop();
        if let Some(area) = drawing_area_crop.upgrade() {
            area.queue_draw();
        }
    });

    let state_background = state.clone();
    let drawing_area_background = drawing_area.downgrade();
    let buttons_background = tool_buttons.to_vec();
    let apply_crop_btn_background = apply_crop.clone();
    let update_toolbar_for_tool_background = update_toolbar_for_tool.clone();
    let sync_picker_for_active_tool_background = sync_picker_for_active_tool.clone();
    let sync_size_control_background = sync_size_control.clone();
    let rebuild_effects_async_background = rebuild_effects_async.clone();
    background.connect_clicked(move |_| {
        let next_tool = {
            let mut state = state_background.lock().unwrap();
            let rebuild = if state.selected_tool == Tool::Background {
                let rebuild = state.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, rebuild)
            } else {
                let rebuild = state.set_tool_without_rebuild(Tool::Background);
                (Tool::Background, rebuild)
            };
            if rebuild.1 {
                rebuild_effects_async_background();
            }
            rebuild.0
        };
        set_active_tool_button(&buttons_background, tool_button_index(next_tool));
        update_toolbar_for_tool_background(next_tool);
        sync_picker_for_active_tool_background();
        sync_size_control_background();
        set_crop_apply_button_state(&apply_crop_btn_background, false, false);
        if let Some(area) = drawing_area_background.upgrade() {
            area.queue_draw();
        }
    });

    let state_pen = state.clone();
    let drawing_area_pen = drawing_area.downgrade();
    let buttons_pen = tool_buttons.to_vec();
    let apply_crop_pen = apply_crop.clone();
    let update_toolbar_pen = update_toolbar_for_tool.clone();
    let sync_size_pen = sync_size_control.clone();
    let rebuild_pen = rebuild_effects_async.clone();
    let window_pen = window.clone();
    pen.connect_clicked(move |_| {
        set_active_tool_button(&buttons_pen, tool_button_index(Tool::Pen));
        if state_pen
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Pen)
        {
            rebuild_pen();
        }
        update_toolbar_pen(Tool::Pen);
        sync_size_pen();
        set_crop_apply_button_state(&apply_crop_pen, false, false);
        {
            let state = state_pen.lock().unwrap();
            update_pen_cursor(&window_pen, &state);
        }
        if let Some(area) = drawing_area_pen.upgrade() {
            area.queue_draw();
        }
    });

    wire_standard_tool(
        arrow,
        Tool::Arrow,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );
    wire_standard_tool(
        line,
        Tool::Line,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );
    wire_standard_tool(
        boxed,
        Tool::Box,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );
    wire_standard_tool(
        circle,
        Tool::Circle,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );
    wire_standard_tool(
        text,
        Tool::Text,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );

    let state_obfuscate = state.clone();
    let drawing_area_obfuscate = drawing_area.downgrade();
    let buttons_obfuscate = tool_buttons.to_vec();
    let apply_crop_btn_obfuscate = apply_crop.clone();
    let update_toolbar_for_tool_obfuscate = update_toolbar_for_tool.clone();
    let sync_size_control_obfuscate = sync_size_control.clone();
    let rebuild_effects_async_obfuscate = rebuild_effects_async.clone();
    obfuscate.connect_clicked(move |_| {
        set_active_tool_button(&buttons_obfuscate, tool_button_index(Tool::Obfuscate));
        {
            let mut state = state_obfuscate.lock().unwrap();
            let changed = state.set_tool_without_rebuild(Tool::Obfuscate);
            state.select_effect_rebuild_pending = false;
            let has_effect_actions = state
                .actions
                .iter()
                .any(EditorState::action_requires_effect_rebuild);
            drop(state);
            if changed || has_effect_actions {
                rebuild_effects_async_obfuscate();
            }
        }
        update_toolbar_for_tool_obfuscate(Tool::Obfuscate);
        sync_size_control_obfuscate();
        set_crop_apply_button_state(&apply_crop_btn_obfuscate, false, false);
        if let Some(area) = drawing_area_obfuscate.upgrade() {
            area.queue_draw();
        }
    });

    wire_standard_tool(
        focus,
        Tool::Focus,
        tool_buttons,
        apply_crop,
        state,
        drawing_area,
        update_toolbar_for_tool,
        sync_size_control,
        rebuild_effects_async,
    );

    let state_number = state.clone();
    let drawing_area_number = drawing_area.downgrade();
    let buttons_number = tool_buttons.to_vec();
    let apply_crop_btn_number = apply_crop.clone();
    let update_toolbar_for_tool_number = update_toolbar_for_tool.clone();
    let sync_size_control_number = sync_size_control.clone();
    let rebuild_effects_async_number = rebuild_effects_async.clone();
    number.connect_clicked(move |_| {
        let next_tool = {
            let mut state = state_number.lock().unwrap();
            if state.selected_tool == Tool::Number {
                let rebuild = state.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, rebuild)
            } else {
                let rebuild = state.set_tool_without_rebuild(Tool::Number);
                (Tool::Number, rebuild)
            }
        };
        if next_tool.1 {
            rebuild_effects_async_number();
        }
        set_active_tool_button(&buttons_number, tool_button_index(next_tool.0));
        update_toolbar_for_tool_number(next_tool.0);
        sync_size_control_number();
        set_crop_apply_button_state(&apply_crop_btn_number, false, false);
        if let Some(area) = drawing_area_number.upgrade() {
            area.queue_draw();
        }
    });

    let state_highlighter = state.clone();
    let drawing_area_highlighter = drawing_area.downgrade();
    let buttons_highlighter = tool_buttons.to_vec();
    let apply_crop_btn_highlighter = apply_crop.clone();
    let update_toolbar_for_tool_highlighter = update_toolbar_for_tool.clone();
    let sync_size_control_highlighter = sync_size_control.clone();
    let window_highlighter = window.clone();
    let rebuild_effects_async_highlighter = rebuild_effects_async.clone();
    highlighter.connect_clicked(move |_| {
        let next_tool = {
            let mut state = state_highlighter.lock().unwrap();
            let rebuild = if state.selected_tool == Tool::Highlighter {
                let rebuild = state.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, rebuild)
            } else {
                let rebuild = state.set_tool_without_rebuild(Tool::Highlighter);
                (Tool::Highlighter, rebuild)
            };
            if rebuild.1 {
                rebuild_effects_async_highlighter();
            }
            rebuild.0
        };
        set_active_tool_button(&buttons_highlighter, tool_button_index(next_tool));
        if !matches!(next_tool, Tool::Highlighter) {
            set_window_cursor_name(&window_highlighter, Some("default"));
        }
        update_toolbar_for_tool_highlighter(next_tool);
        sync_size_control_highlighter();
        set_crop_apply_button_state(&apply_crop_btn_highlighter, false, false);
        if let Some(area) = drawing_area_highlighter.upgrade() {
            area.queue_draw();
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn wire_standard_tool(
    button: &Button,
    tool: Tool,
    tool_buttons: &[Button],
    apply_crop: &Button,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    update_toolbar_for_tool: &Rc<dyn Fn(Tool)>,
    sync_size_control: &Rc<dyn Fn()>,
    rebuild_effects_async: &Rc<dyn Fn()>,
) {
    let state = state.clone();
    let drawing_area = drawing_area.downgrade();
    let tool_buttons = tool_buttons.to_vec();
    let apply_crop = apply_crop.clone();
    let update_toolbar_for_tool = update_toolbar_for_tool.clone();
    let sync_size_control = sync_size_control.clone();
    let rebuild_effects_async = rebuild_effects_async.clone();
    button.connect_clicked(move |_| {
        set_active_tool_button(&tool_buttons, tool_button_index(tool));
        if state.lock().unwrap().set_tool_without_rebuild(tool) {
            rebuild_effects_async();
        }
        update_toolbar_for_tool(tool);
        sync_size_control();
        set_crop_apply_button_state(&apply_crop, false, false);
        if let Some(area) = drawing_area.upgrade() {
            area.queue_draw();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_mode_switches_preserve_special_toggle_policies() {
        let source = include_str!("tools.rs");
        assert!(
            source.contains("if state.selected_tool == Tool::Crop")
                && source.contains("state.ensure_crop_selection_initialized()")
                && source.contains("if state.selected_tool == Tool::Number")
                && source.contains("if state.selected_tool == Tool::Highlighter")
                && source.contains("state.select_effect_rebuild_pending = false")
                && source.contains("any(EditorState::action_requires_effect_rebuild)"),
            "crop, number, highlighter, and obfuscation must retain their distinct tool-mode policies"
        );
    }
}
