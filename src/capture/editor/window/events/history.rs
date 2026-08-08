use gtk4::{prelude::*, Button, DrawingArea};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::super::state::EditorState;

pub(super) fn wire_history_buttons(
    undo_btn: &Button,
    redo_btn: &Button,
    delete_selected_btn: &Button,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    rebuild_effects_async: &Rc<dyn Fn()>,
    sync_size_control: &Rc<dyn Fn()>,
    sync_select_inspector: &Rc<dyn Fn()>,
) {
    let state_undo = state.clone();
    let drawing_area_undo = drawing_area.downgrade();
    let sync_size_control_undo = sync_size_control.clone();
    let rebuild_effects_async_undo = rebuild_effects_async.clone();
    undo_btn.connect_clicked(move |_| {
        let changed = state_undo.lock().unwrap().undo_without_rebuild();
        if changed {
            rebuild_effects_async_undo();
            sync_size_control_undo();
            if let Some(area) = drawing_area_undo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_redo = state.clone();
    let drawing_area_redo = drawing_area.downgrade();
    let sync_size_control_redo = sync_size_control.clone();
    let rebuild_effects_async_redo = rebuild_effects_async.clone();
    redo_btn.connect_clicked(move |_| {
        let changed = state_redo.lock().unwrap().redo_without_rebuild();
        if changed {
            rebuild_effects_async_redo();
            sync_size_control_redo();
            if let Some(area) = drawing_area_redo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_delete_selected = state.clone();
    let drawing_area_delete_selected = drawing_area.downgrade();
    let rebuild_effects_async_delete = rebuild_effects_async.clone();
    let sync_select_inspector_delete = sync_select_inspector.clone();
    delete_selected_btn.connect_clicked(move |_| {
        if state_delete_selected
            .lock()
            .unwrap()
            .remove_selected_action_without_rebuild()
        {
            rebuild_effects_async_delete();
            sync_select_inspector_delete();
            if let Some(area) = drawing_area_delete_selected.upgrade() {
                area.queue_draw();
            }
        }
    });
}
