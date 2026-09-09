use gtk4::prelude::*;

use crate::recording::editor::model::{MotionEffectTransformTiming, MOTION_SCALE_PRESETS};

use super::super::{MotionModeParts, MotionSession};
use super::{Redraw, RequestLivePreview, RequestTransitionPreview};

pub(super) fn install(
    parts: &MotionModeParts,
    session: &MotionSession,
    redraw: Redraw,
    request_live_preview: RequestLivePreview,
    request_transition_preview: RequestTransitionPreview,
) {
    for (i, (_, scale)) in MOTION_SCALE_PRESETS.iter().enumerate() {
        let chip = parts.transform.scale_chips[i].clone();
        chip.connect_clicked({
            let session = session.runtime.clone();
            let redraw = redraw.clone();
            let request_transition_preview = request_transition_preview.clone();
            let syncing = parts.shared.inspector_syncing.clone();
            let scale = *scale;
            move |_| {
                if syncing.get() {
                    return;
                }
                let segment_start = {
                    let runtime = session.borrow();
                    runtime
                        .motion
                        .selected_segment()
                        .map(|segment| segment.start)
                };
                session.borrow_mut().motion.set_selected_end_scale(scale);
                match segment_start {
                    Some(start) => request_transition_preview(start),
                    None => redraw(),
                }
            }
        });
    }

    parts.transform.intensity_slider.connect_value_changed({
        let session = session.runtime.clone();
        let intensity_value = parts.transform.intensity_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_intensity(value);
            intensity_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });

    for (axis, slider, value_label) in [
        (
            0_u8,
            parts.transform.zoom_anchor_x_slider.clone(),
            parts.transform.zoom_anchor_x_value.clone(),
        ),
        (
            1_u8,
            parts.transform.zoom_anchor_y_slider.clone(),
            parts.transform.zoom_anchor_y_value.clone(),
        ),
    ] {
        let session = session.runtime.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        slider.connect_value_changed(move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            let mut runtime = session.borrow_mut();
            let (mut x, mut y) = runtime
                .motion
                .selected_segment()
                .map_or((0.5, 0.5), |segment| {
                    (segment.zoom_anchor_x, segment.zoom_anchor_y)
                });
            if axis == 0 {
                x = value;
            } else {
                y = value;
            }
            runtime.motion.set_selected_zoom_anchor(x, y);
            drop(runtime);
            value_label.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        });
    }

    parts.transform.yaw_slider.connect_value_changed({
        let session = session.runtime.clone();
        let yaw_value = parts.transform.yaw_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_yaw(value);
            yaw_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.transform.pitch_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pitch_value = parts.transform.pitch_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pitch(value);
            pitch_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.transform.roll_slider.connect_value_changed({
        let session = session.runtime.clone();
        let roll_value = parts.transform.roll_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_roll(value);
            roll_value.set_label(&format!("{:.0}°", value));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.transform.perspective_slider.connect_value_changed({
        let session = session.runtime.clone();
        let perspective_value = parts.transform.perspective_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            session.borrow_mut().motion.set_selected_perspective(value);
            perspective_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });
    parts.transform.pos_x_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pos_x_value = parts.transform.pos_x_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pos_x(value);
            pos_x_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.transform.pos_y_slider.connect_value_changed({
        let session = session.runtime.clone();
        let pos_y_value = parts.transform.pos_y_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session.borrow_mut().motion.set_selected_end_pos_y(value);
            pos_y_value.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });
    parts.transform.ease_slider.connect_value_changed({
        let session = session.runtime.clone();
        let ease_value = parts.transform.ease_value.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let transition_ms = slider.value().round() as u32;
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session
                .borrow_mut()
                .motion
                .set_selected_transition_ms(transition_ms);
            ease_value.set_label(&format!("{transition_ms}ms"));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        }
    });

    for (axis, slider, value_label) in [
        (
            0_u8,
            parts.transform.easing_x1_slider.clone(),
            parts.transform.easing_x1_value.clone(),
        ),
        (
            1_u8,
            parts.transform.easing_y1_slider.clone(),
            parts.transform.easing_y1_value.clone(),
        ),
        (
            2_u8,
            parts.transform.easing_x2_slider.clone(),
            parts.transform.easing_x2_value.clone(),
        ),
        (
            3_u8,
            parts.transform.easing_y2_slider.clone(),
            parts.transform.easing_y2_value.clone(),
        ),
    ] {
        let session = session.runtime.clone();
        let request_transition_preview = request_transition_preview.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        slider.connect_value_changed(move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            let mut runtime = session.borrow_mut();
            let mut timing = runtime.motion.transform_timing;
            match axis {
                0 => timing.easing_x1 = value,
                1 => timing.easing_y1 = value,
                2 => timing.easing_x2 = value,
                _ => timing.easing_y2 = value,
            }
            runtime.motion.set_transform_timing(timing);
            drop(runtime);
            value_label.set_label(&format!("{:.0}%", value * 100.0));
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => request_live_preview(),
            }
        });
    }

    parts.transform.reset_timing_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let request_transition_preview = request_transition_preview.clone();
        move |_| {
            let segment_start = {
                let runtime = session.borrow();
                runtime
                    .motion
                    .selected_segment()
                    .map(|segment| segment.start)
            };
            session
                .borrow_mut()
                .motion
                .set_transform_timing(MotionEffectTransformTiming::default());
            match segment_start {
                Some(start) => request_transition_preview(start),
                None => redraw(),
            }
        }
    });
}
