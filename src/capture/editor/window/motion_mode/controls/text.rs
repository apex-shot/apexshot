use gtk4::{prelude::*, GestureDrag};
use std::cell::Cell;
use std::rc::Rc;

use crate::recording::editor::model::{DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y};

use super::super::{MotionModeParts, MotionSession};
use super::{RequestLivePreview, RequestTextTransitionPreview};

pub(super) fn install(
    parts: &MotionModeParts,
    session: &MotionSession,
    request_live_preview: RequestLivePreview,
    request_text_transition_preview: RequestTextTransitionPreview,
) {
    parts.text.text_entry.connect_changed({
        let session = session.runtime.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |entry| {
            if syncing.get() {
                return;
            }
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime
                    .motion
                    .set_selected_text_value(entry.text().to_string());
            }
            request_live_preview();
        }
    });

    for (animation, button) in &parts.text.text_anim_buttons {
        let animation = *animation;
        button.connect_toggled({
            let session = session.runtime.clone();
            let request_text_transition_preview = request_text_transition_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                let text_start = {
                    let runtime = session.borrow();
                    runtime
                        .motion
                        .selected_text_segment()
                        .map(|segment| (segment.start, segment.typewriter_time))
                };
                {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.set_selected_text_animation(animation);
                }
                if let Some((start, typewriter_time)) = text_start {
                    request_text_transition_preview(start, typewriter_time);
                }
            }
        });
    }

    for (scope, button) in &parts.text.text_scope_buttons {
        let scope = *scope;
        button.connect_toggled({
            let session = session.runtime.clone();
            let request_text_transition_preview = request_text_transition_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                let text_start = {
                    let runtime = session.borrow();
                    runtime
                        .motion
                        .selected_text_segment()
                        .map(|segment| (segment.start, segment.typewriter_time))
                };
                {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.set_selected_text_scope(scope);
                }
                if let Some((start, typewriter_time)) = text_start {
                    request_text_transition_preview(start, typewriter_time);
                }
            }
        });
    }

    parts.text.text_pos_x_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_pos_x_value = parts.text.text_pos_x_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let pos_y = session
                .borrow()
                .motion
                .selected_text_segment()
                .map(|segment| segment.pos_y)
                .unwrap_or(DEFAULT_MOTION_TEXT_POS_Y);
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_pos(value, pos_y);
            }
            text_pos_x_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.text.text_pos_y_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_pos_y_value = parts.text.text_pos_y_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let pos_x = session
                .borrow()
                .motion
                .selected_text_segment()
                .map(|segment| segment.pos_x)
                .unwrap_or(DEFAULT_MOTION_TEXT_POS_X);
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_pos(pos_x, value);
            }
            text_pos_y_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.text.text_size_slider.connect_value_changed({
        let session = session.runtime.clone();
        let text_size_value = parts.text.text_size_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_size(value);
            }
            text_size_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });

    let place_text = {
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let text_pos_x_slider = parts.text.text_pos_x_slider.clone();
        let text_pos_x_value = parts.text.text_pos_x_value.clone();
        let text_pos_y_slider = parts.text.text_pos_y_slider.clone();
        let text_pos_y_value = parts.text.text_pos_y_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        Rc::new(move |x: f64, y: f64| {
            let runtime = session.borrow();
            if runtime.motion.selected_text.is_none() {
                return;
            }
            let width = preview.allocated_width().max(1) as f64;
            let height = preview.allocated_height().max(1) as f64;
            let Some(card) = runtime.card.as_ref() else {
                return;
            };
            let transform = runtime.motion.sample(runtime.motion.playhead);
            let zoom_anchor = runtime.motion.zoom_anchor_at(runtime.motion.playhead);
            let stage = super::super::super::motion_render::MotionStage::preview(
                width,
                height,
                runtime.motion.frame.preset.aspect(),
            );
            let padding = runtime.motion.appearance.background_padding;
            let (pos_x, pos_y) =
                super::super::super::motion_render::view_point_to_motion_text_position(
                    card,
                    stage,
                    padding,
                    transform,
                    zoom_anchor,
                    x,
                    y,
                );
            drop(runtime);
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_pos(pos_x, pos_y);
            }
            syncing.set(true);
            text_pos_x_slider.set_value(pos_x);
            text_pos_x_value.set_label(&format!("{:.0}%", pos_x * 100.0));
            text_pos_y_slider.set_value(pos_y);
            text_pos_y_value.set_label(&format!("{:.0}%", pos_y * 100.0));
            syncing.set(false);
            request_live_preview();
        })
    };
    let text_drag_armed = Rc::new(Cell::new(false));
    let preview_drag = GestureDrag::new();
    preview_drag.set_button(1);
    preview_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let text_drag_armed = text_drag_armed.clone();
        move |_, x, y| {
            let runtime = session.borrow();
            let hit = runtime
                .motion
                .selected_text_segment()
                .and_then(|segment| {
                    runtime.card.as_ref().map(|card| {
                        let width = preview.allocated_width().max(1) as f64;
                        let height = preview.allocated_height().max(1) as f64;
                        super::super::super::motion_render::motion_text_contains_view_point(
                            card,
                            super::super::super::motion_render::MotionStage::preview(
                                width,
                                height,
                                runtime.motion.frame.preset.aspect(),
                            ),
                            runtime.motion.appearance.background_padding,
                            runtime.motion.sample(runtime.motion.playhead),
                            runtime.motion.zoom_anchor_at(runtime.motion.playhead),
                            segment,
                            runtime.motion.playhead,
                            x,
                            y,
                        )
                    })
                })
                .unwrap_or(false);
            text_drag_armed.set(hit);
        }
    });
    preview_drag.connect_drag_update({
        let place_text = place_text.clone();
        let text_drag_armed = text_drag_armed.clone();
        move |gesture, offset_x, offset_y| {
            if !text_drag_armed.get() {
                return;
            }
            let Some((start_x, start_y)) = gesture.start_point() else {
                return;
            };
            place_text(start_x + offset_x, start_y + offset_y);
        }
    });
    preview_drag.connect_drag_end({
        let text_drag_armed = text_drag_armed.clone();
        move |_, _, _| text_drag_armed.set(false)
    });
    parts.shell.preview.add_controller(preview_drag);
}
