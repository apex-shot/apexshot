use gtk4::{prelude::*, GestureDrag};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::recording::editor::model::{MotionState, MotionTextSegment, MotionTransform};
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::super::{MotionModeParts, MotionRuntime, MotionSession};
use super::{Redraw, RequestLivePreview, RequestTextTransitionPreview};

/// Everything needed to map between preview view coordinates and a title's
/// own reference rectangle.
struct TextPlacementContext {
    card: gtk4::cairo::ImageSurface,
    stage: super::super::super::motion_render::MotionStage,
    padding: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    card_scale: f64,
    time: f64,
}

impl TextPlacementContext {
    fn placement(
        &self,
        segment: &MotionTextSegment,
        time: f64,
    ) -> Option<super::super::super::motion_render::MotionTextPlacement> {
        let context = gtk4::cairo::Context::new(&self.card).ok()?;
        super::super::super::motion_render::motion_text_placement(
            &context,
            &self.card,
            self.stage,
            segment,
            time,
            self.card_scale,
            self.transform,
            self.zoom_anchor,
            self.padding,
        )
    }

    fn anchor_view_point(&self, segment: &MotionTextSegment) -> Option<(f64, f64)> {
        Some(self.placement(segment, segment.start)?.anchor_view_point())
    }

    /// The normalized position that puts the title's anchor at `(x, y)` in
    /// preview coordinates.
    fn position_at(&self, segment: &MotionTextSegment, x: f64, y: f64) -> Option<(f64, f64)> {
        self.placement(segment, segment.start)?.position_at(x, y)
    }
}

/// The pointer's grab at drag start: the anchor's view point and where the
/// pointer started. Keeping both means the title holds its offset under the
/// pointer instead of jumping to centre on the first update.
#[derive(Clone, Copy)]
struct TextDragGrab {
    anchor_x: f64,
    anchor_y: f64,
    pointer_x: f64,
    pointer_y: f64,
}

pub(super) fn install(
    parts: &MotionModeParts,
    session: &MotionSession,
    redraw: Redraw,
    request_live_preview: RequestLivePreview,
    request_text_transition_preview: RequestTextTransitionPreview,
) {
    // The Text page owns its own add/delete so it works with nothing
    // selected, exactly like the timeline's add button.
    parts.text.text_add_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let request_text_transition_preview = request_text_transition_preview.clone();
        move |_| {
            let new_text = {
                let mut runtime = session.borrow_mut();
                let playhead = runtime.motion.playhead;
                runtime.begin_motion_edit();
                runtime.motion.add_text_at(playhead).and_then(|index| {
                    runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|s| (s.start, s.entrance_seconds()))
                })
            };
            redraw();
            if let Some((start, entrance_seconds)) = new_text {
                request_text_transition_preview(start, entrance_seconds);
            }
        }
    });
    parts.text.text_delete_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.remove_selected();
            drop(runtime);
            redraw();
        }
    });
    parts.text.text_view.buffer().connect_changed({
        let session = session.runtime.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |buffer| {
            if syncing.get() {
                return;
            }
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_value(text);
            }
            request_live_preview();
        }
    });

    parts.text.text_font_picker.connect_changed({
        let session = session.runtime.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |family| {
            if syncing.get() {
                return;
            }
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_font_family(family);
            }
            request_live_preview();
        }
    });

    connect_text_toggle(
        &parts.text.text_bold_btn,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, active| motion.set_selected_text_bold(active),
    );
    connect_text_toggle(
        &parts.text.text_italic_btn,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, active| motion.set_selected_text_italic(active),
    );
    connect_text_toggle(
        &parts.text.text_shadow_btn,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, active| motion.set_selected_text_shadow(active),
    );

    parts.text.text_color_picker.connect_changed({
        let session = session.runtime.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |(red, green, blue)| {
            if syncing.get() {
                return;
            }
            let mut runtime = session.borrow_mut();
            let alpha = runtime
                .motion
                .selected_text_segment()
                .map(|segment| segment.format.color[3])
                .unwrap_or(1.0);
            runtime.begin_motion_edit();
            runtime.motion.set_selected_text_color([
                f64::from(red) / 255.0,
                f64::from(green) / 255.0,
                f64::from(blue) / 255.0,
                alpha,
            ]);
            drop(runtime);
            request_live_preview();
        }
    });

    connect_text_value(
        &parts.text.text_opacity_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_opacity(value / 100.0),
    );
    connect_text_value(
        &parts.text.text_width_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_wrap_width(value),
    );
    connect_text_value(
        &parts.text.text_size_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_size(value),
    );
    connect_text_value(
        &parts.text.text_line_spacing_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_line_spacing(value),
    );
    connect_text_value(
        &parts.text.text_letter_spacing_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_letter_spacing(value),
    );
    connect_text_value(
        &parts.text.text_rotation_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_rotation(value),
    );
    connect_text_value(
        &parts.text.text_outline_slider,
        &parts.shared.inspector_syncing,
        &session.runtime,
        &request_live_preview,
        |motion, value| motion.set_selected_text_outline_width(value),
    );

    for (alignment, button) in &parts.text.text_alignment_buttons {
        let alignment = *alignment;
        button.connect_toggled({
            let session = session.runtime.clone();
            let request_live_preview = request_live_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.set_selected_text_alignment(alignment);
                }
                request_live_preview();
            }
        });
    }

    for (space, button) in &parts.text.text_attach_buttons {
        let space = *space;
        button.connect_toggled({
            let session = session.runtime.clone();
            let preview = parts.shell.preview.clone();
            let text_pos_pad = parts.text.text_pos_pad.clone();
            let text_pos_readout = parts.text.text_pos_readout.clone();
            let request_live_preview = request_live_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                let Some(context) = preview_placement_context(&session, &preview) else {
                    return;
                };
                let (pos_x, pos_y) = {
                    let runtime = session.borrow();
                    let Some(segment) = runtime.motion.selected_text_segment() else {
                        return;
                    };
                    let Some((anchor_x, anchor_y)) = context.anchor_view_point(segment) else {
                        return;
                    };
                    let mut target = segment.clone();
                    target.annotation_coordinate_space = space;
                    let Some(position) = context.position_at(&target, anchor_x, anchor_y) else {
                        return;
                    };
                    position
                };
                {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime
                        .motion
                        .set_selected_text_attachment(space, pos_x, pos_y);
                }
                syncing.set(true);
                text_pos_pad.set_attachment(space.is_canvas());
                text_pos_pad.set_text_pos(pos_x, pos_y);
                text_pos_readout
                    .set_label(&super::super::build::motion_text_pos_readout(pos_x, pos_y));
                syncing.set(false);
                request_live_preview();
            }
        });
    }

    parts.text.text_transition_slider.connect_value_changed({
        let session = session.runtime.clone();
        let request_text_transition_preview = request_text_transition_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = (slider.value() / 1000.0).max(0.0);
            let entrance = {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_transition_duration(value);
                runtime
                    .motion
                    .selected_text_segment()
                    .map(|segment| (segment.start, segment.entrance_seconds()))
            };
            if let Some((start, entrance_seconds)) = entrance {
                request_text_transition_preview(start, entrance_seconds);
            }
        }
    });
    parts.text.text_typewriter_slider.connect_value_changed({
        let session = session.runtime.clone();
        let request_text_transition_preview = request_text_transition_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = (slider.value() / 1000.0).max(0.0);
            let entrance = {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_typewriter_time(value);
                runtime
                    .motion
                    .selected_text_segment()
                    .map(|segment| (segment.start, segment.entrance_seconds()))
            };
            if let Some((start, entrance_seconds)) = entrance {
                request_text_transition_preview(start, entrance_seconds);
            }
        }
    });

    for (animation, button) in &parts.text.text_anim_buttons {
        let animation = *animation;
        button.connect_toggled({
            let session = session.runtime.clone();
            let redraw = redraw.clone();
            let request_text_transition_preview = request_text_transition_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            move |button| {
                if syncing.get() || !button.is_active() {
                    return;
                }
                let entrance = {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.set_selected_text_animation(animation);
                    runtime
                        .motion
                        .selected_text_segment()
                        .map(|segment| (segment.start, segment.entrance_seconds()))
                };
                redraw();
                if let Some((start, entrance_seconds)) = entrance {
                    request_text_transition_preview(start, entrance_seconds);
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
                let entrance = {
                    let mut runtime = session.borrow_mut();
                    runtime.begin_motion_edit();
                    runtime.motion.set_selected_text_scope(scope);
                    runtime
                        .motion
                        .selected_text_segment()
                        .map(|segment| (segment.start, segment.entrance_seconds()))
                };
                if let Some((start, entrance_seconds)) = entrance {
                    request_text_transition_preview(start, entrance_seconds);
                }
            }
        });
    }

    parts.text.text_pos_pad.connect_value_changed({
        let session = session.runtime.clone();
        let text_pos_readout = parts.text.text_pos_readout.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |pos_x, pos_y| {
            if syncing.get() {
                return;
            }
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_pos(pos_x, pos_y);
            }
            text_pos_readout.set_label(&super::super::build::motion_text_pos_readout(pos_x, pos_y));
            request_live_preview();
        }
    });

    let text_drag_grab = Rc::new(Cell::new(None::<TextDragGrab>));
    let preview_drag = GestureDrag::new();
    preview_drag.set_button(1);
    preview_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let text_drag_grab = text_drag_grab.clone();
        move |_, x, y| {
            text_drag_grab.set(None);
            let Some(context) = preview_placement_context(&session, &preview) else {
                return;
            };
            let runtime = session.borrow();
            let Some(segment) = runtime.motion.selected_text_segment() else {
                return;
            };
            let Some(placement) = context.placement(segment, context.time) else {
                return;
            };
            if !placement.contains_stage_point(x, y, placement.local_slop(6.0)) {
                return;
            }
            let (anchor_x, anchor_y) = placement.anchor_view_point();
            text_drag_grab.set(Some(TextDragGrab {
                anchor_x,
                anchor_y,
                pointer_x: x,
                pointer_y: y,
            }));
        }
    });
    preview_drag.connect_drag_update({
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let text_pos_pad = parts.text.text_pos_pad.clone();
        let text_pos_readout = parts.text.text_pos_readout.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        let text_drag_grab = text_drag_grab.clone();
        move |gesture, offset_x, offset_y| {
            let Some(grab) = text_drag_grab.get() else {
                return;
            };
            let Some((start_x, start_y)) = gesture.start_point() else {
                return;
            };
            let pointer_x = start_x + offset_x;
            let pointer_y = start_y + offset_y;
            let Some(context) = preview_placement_context(&session, &preview) else {
                return;
            };
            let position = {
                let runtime = session.borrow();
                let Some(segment) = runtime.motion.selected_text_segment() else {
                    return;
                };
                context.position_at(
                    segment,
                    grab.anchor_x + pointer_x - grab.pointer_x,
                    grab.anchor_y + pointer_y - grab.pointer_y,
                )
            };
            let Some((pos_x, pos_y)) = position else {
                return;
            };
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.set_selected_text_pos(pos_x, pos_y);
            }
            syncing.set(true);
            text_pos_pad.set_text_pos(pos_x, pos_y);
            text_pos_readout.set_label(&super::super::build::motion_text_pos_readout(pos_x, pos_y));
            syncing.set(false);
            request_live_preview();
        }
    });
    preview_drag.connect_drag_end({
        let text_drag_grab = text_drag_grab.clone();
        move |_, _, _| text_drag_grab.set(None)
    });
    parts.shell.preview.add_controller(preview_drag);
}

/// The preview's render geometry for the selected title, or `None` while the
/// card or a selection is missing.
fn preview_placement_context(
    session: &Rc<RefCell<MotionRuntime>>,
    preview: &gtk4::DrawingArea,
) -> Option<TextPlacementContext> {
    let width = f64::from(preview.allocated_width().max(1));
    let height = f64::from(preview.allocated_height().max(1));
    let runtime = session.borrow();
    let (card, card_scale) = match runtime.card_preview.as_ref() {
        Some(card) => (card.clone(), runtime.card_scale),
        None => (runtime.card.clone()?, 1.0),
    };
    let time = super::super::preview::preview_time(&runtime);
    let stage = super::super::super::motion_render::MotionStage::preview(
        width,
        height,
        runtime.motion.frame.effective_aspect(),
    );
    let context = TextPlacementContext {
        card,
        stage,
        padding: runtime.motion.appearance.effective_padding(),
        transform: runtime.motion.sample(time),
        zoom_anchor: runtime.motion.zoom_anchor_at(time),
        card_scale,
        time,
    };
    Some(context)
}

fn connect_text_value(
    slider: &FillSlider,
    syncing: &Rc<Cell<bool>>,
    session: &Rc<RefCell<MotionRuntime>>,
    request_live_preview: &RequestLivePreview,
    apply: impl Fn(&mut MotionState, f64) + 'static,
) {
    slider.connect_value_changed({
        let session = session.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                apply(&mut runtime.motion, value);
            }
            request_live_preview();
        }
    });
}

fn connect_text_toggle(
    button: &gtk4::ToggleButton,
    syncing: &Rc<Cell<bool>>,
    session: &Rc<RefCell<MotionRuntime>>,
    request_live_preview: &RequestLivePreview,
    apply: impl Fn(&mut MotionState, bool) + 'static,
) {
    button.connect_toggled({
        let session = session.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() {
                return;
            }
            let active = button.is_active();
            {
                let mut runtime = session.borrow_mut();
                runtime.begin_motion_edit();
                apply(&mut runtime.motion, active);
            }
            request_live_preview();
        }
    });
}
