//! Crop apply/reset button wiring for the editor inspector (PR 10.8).
//!
//! Owns only the apply and reset action buttons plus their dimension/layout
//! refresh. Canvas crop dragging stays in the interaction controller until
//! shared drag state is extracted. Tool-mode Crop activation lives in
//! `tools.rs`.

use gtk4::{prelude::*, Button, DrawingArea};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{state::EditorState, ui_support::set_crop_apply_button_state};

/// Wire crop apply/reset buttons and the layout/field refresh they trigger.
pub(super) fn wire_crop_action_buttons(
    apply_crop_btn: &Button,
    crop_reset_btn: &Button,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    update_canvas_content_size: &Rc<dyn Fn()>,
    update_crop_size_fields: &Rc<dyn Fn()>,
) {
    let state_apply_crop = state.clone();
    let drawing_area_apply_crop = drawing_area.downgrade();
    let apply_crop_btn_click = apply_crop_btn.clone();
    let update_canvas_content_size_apply = update_canvas_content_size.clone();
    let update_crop_size_fields_apply_crop = update_crop_size_fields.clone();
    apply_crop_btn.connect_clicked(move |_| {
        let apply_result = {
            let mut st = state_apply_crop.lock().unwrap();
            st.apply_crop_selection()
        };

        match apply_result {
            Ok(true) => {
                update_canvas_content_size_apply();
                set_crop_apply_button_state(&apply_crop_btn_click, true, false);
                update_crop_size_fields_apply_crop();
                if let Some(area) = drawing_area_apply_crop.upgrade() {
                    area.queue_draw();
                }
            }
            Ok(false) => {
                set_crop_apply_button_state(&apply_crop_btn_click, true, false);
                update_crop_size_fields_apply_crop();
            }
            Err(e) => {
                eprintln!("Failed to apply crop: {e}");
            }
        }
    });

    let state_reset_crop = state.clone();
    let drawing_area_reset_crop = drawing_area.downgrade();
    let update_crop_size_fields_reset_crop = update_crop_size_fields.clone();
    let apply_crop_btn_reset = apply_crop_btn.clone();
    crop_reset_btn.connect_clicked(move |_| {
        {
            let mut st = state_reset_crop.lock().unwrap();
            st.reset_crop_interaction();
        }
        set_crop_apply_button_state(&apply_crop_btn_reset, true, false);
        update_crop_size_fields_reset_crop();
        if let Some(area) = drawing_area_reset_crop.upgrade() {
            area.queue_draw();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn crop_action_buttons_refresh_layout_and_fields() {
        let source = include_str!("crop.rs");
        assert!(
            source.contains("st.apply_crop_selection()")
                && source.contains("update_canvas_content_size_apply()")
                && source.contains("st.reset_crop_interaction()")
                && source.contains("set_crop_apply_button_state(&apply_crop_btn_click, true, false)")
                && source.contains("set_crop_apply_button_state(&apply_crop_btn_reset, true, false)")
                && source.contains("update_crop_size_fields_apply_crop()")
                && source.contains("update_crop_size_fields_reset_crop()"),
            "crop apply must refresh canvas layout on success; apply and reset must clear the apply button and size fields"
        );
    }
}
