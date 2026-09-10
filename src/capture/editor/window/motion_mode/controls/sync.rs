use gtk4::prelude::*;
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::MotionState;

use super::super::widgets::format_duration_label;
use super::super::{MotionModeParts, MotionSession};
use super::{Redraw, RequestLivePreview};

pub(super) fn make_redraw(parts: &MotionModeParts, session: &MotionSession) -> Redraw {
    let preview = parts.shell.preview.clone();
    let ruler = parts.timeline.ruler.clone();
    let source_track = parts.timeline.source_track.clone();
    let motion_track = parts.timeline.motion_track.clone();
    let playhead_overlay = parts.timeline.playhead_overlay.clone();
    let playhead_clock = parts.timeline.playhead_clock.clone();
    let duration_clock = parts.timeline.duration_clock.clone();
    let play_btn = parts.timeline.play_btn.clone();
    let session = session.runtime.clone();
    let text_track = parts.timeline.text_track.clone();
    let blur_slider = parts.shared.blur_slider.clone();
    let blur_value = parts.shared.blur_value.clone();
    let blur_shutter_slider = parts.shared.blur_shutter_slider.clone();
    let blur_shutter_value = parts.shared.blur_shutter_value.clone();
    let blur_trail_slider = parts.shared.blur_trail_slider.clone();
    let blur_trail_value = parts.shared.blur_trail_value.clone();
    let clip_box = parts.transform.clip_box.clone();
    let text_box = parts.text.text_box.clone();
    let text_entry = parts.text.text_entry.clone();
    let text_pos_x_slider = parts.text.text_pos_x_slider.clone();
    let text_pos_x_value = parts.text.text_pos_x_value.clone();
    let text_pos_y_slider = parts.text.text_pos_y_slider.clone();
    let text_pos_y_value = parts.text.text_pos_y_value.clone();
    let text_size_slider = parts.text.text_size_slider.clone();
    let text_size_value = parts.text.text_size_value.clone();
    let text_anim_buttons = parts.text.text_anim_buttons.clone();
    let text_scope_buttons = parts.text.text_scope_buttons.clone();
    let clip_hint = parts.shared.clip_hint.clone();
    let scale_slider = parts.transform.scale_slider.clone();
    let intensity_slider = parts.transform.intensity_slider.clone();
    let intensity_value = parts.transform.intensity_value.clone();
    let zoom_anchor_x_slider = parts.transform.zoom_anchor_x_slider.clone();
    let zoom_anchor_x_value = parts.transform.zoom_anchor_x_value.clone();
    let zoom_anchor_y_slider = parts.transform.zoom_anchor_y_slider.clone();
    let zoom_anchor_y_value = parts.transform.zoom_anchor_y_value.clone();
    let yaw_slider = parts.transform.yaw_slider.clone();
    let yaw_value = parts.transform.yaw_value.clone();
    let pitch_slider = parts.transform.pitch_slider.clone();
    let pitch_value = parts.transform.pitch_value.clone();
    let roll_slider = parts.transform.roll_slider.clone();
    let roll_value = parts.transform.roll_value.clone();
    let perspective_slider = parts.transform.perspective_slider.clone();
    let perspective_value = parts.transform.perspective_value.clone();
    let position_pad = parts.transform.position_pad.clone();
    let pos_x_slider = parts.transform.pos_x_slider.clone();
    let pos_x_value = parts.transform.pos_x_value.clone();
    let pos_y_slider = parts.transform.pos_y_slider.clone();
    let pos_y_value = parts.transform.pos_y_value.clone();
    let ease_slider = parts.transform.ease_slider.clone();
    let ease_value = parts.transform.ease_value.clone();
    let easing_x1_slider = parts.transform.easing_x1_slider.clone();
    let easing_x1_value = parts.transform.easing_x1_value.clone();
    let easing_y1_slider = parts.transform.easing_y1_slider.clone();
    let easing_y1_value = parts.transform.easing_y1_value.clone();
    let easing_x2_slider = parts.transform.easing_x2_slider.clone();
    let easing_x2_value = parts.transform.easing_x2_value.clone();
    let easing_y2_slider = parts.transform.easing_y2_slider.clone();
    let easing_y2_value = parts.transform.easing_y2_value.clone();
    let reset_timing_btn = parts.transform.reset_timing_btn.clone();
    let delete_btn = parts.shared.delete_btn.clone();
    let syncing = parts.shared.inspector_syncing.clone();
    Rc::new(move || {
        let runtime = session.borrow();
        playhead_clock.set_text(&super::super::super::motion_timeline::format_clock(
            runtime.motion.playhead,
        ));
        duration_clock.set_text(&super::super::super::motion_timeline::format_clock(
            runtime.motion.duration,
        ));
        play_btn.set_tooltip_text(Some(&if runtime.playing {
            t("Pause")
        } else {
            t("Play")
        }));
        if let Some(image) = play_btn
            .child()
            .and_then(|child| child.downcast::<gtk4::Image>().ok())
        {
            image.set_icon_name(Some(if runtime.playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            }));
        }
        let selected = runtime.motion.selected_segment().cloned();
        let selected_text = runtime.motion.selected_text_segment().cloned();
        let blur = runtime.motion.motion_blur;
        let blur_settings = runtime.motion.motion_blur_settings.clamped();
        let perspective_intensity = runtime.motion.perspective_intensity;
        let transform_timing = runtime.motion.transform_timing;
        drop(runtime);
        syncing.set(true);
        blur_slider.set_value(blur);
        blur_value.set_label(&format!("{:.0}%", blur * 100.0));
        blur_shutter_slider.set_value(blur_settings.shutter_angle);
        blur_shutter_value.set_label(&format!("{:.0}°", blur_settings.shutter_angle));
        blur_trail_slider.set_value(blur_settings.transform_trail_opacity);
        blur_trail_value.set_label(&format!(
            "{:.0}%",
            blur_settings.transform_trail_opacity * 100.0
        ));
        easing_x1_slider.set_value(transform_timing.easing_x1);
        easing_x1_value.set_label(&format!("{:.0}%", transform_timing.easing_x1 * 100.0));
        easing_y1_slider.set_value(transform_timing.easing_y1);
        easing_y1_value.set_label(&format!("{:.0}%", transform_timing.easing_y1 * 100.0));
        easing_x2_slider.set_value(transform_timing.easing_x2);
        easing_x2_value.set_label(&format!("{:.0}%", transform_timing.easing_x2 * 100.0));
        easing_y2_slider.set_value(transform_timing.easing_y2);
        easing_y2_value.set_label(&format!("{:.0}%", transform_timing.easing_y2 * 100.0));
        let has_clip = selected.is_some();
        let has_text = selected_text.is_some();
        reset_timing_btn.set_sensitive(has_clip);
        clip_box.set_visible(has_clip);
        text_box.set_visible(has_text);
        clip_hint.set_visible(!has_clip && !has_text);
        if let Some(segment) = selected_text {
            if !text_entry.has_focus() {
                text_entry.set_text(&segment.text);
            }
            text_pos_x_slider.set_value(segment.pos_x);
            text_pos_x_value.set_label(&format!("{:.0}%", segment.pos_x * 100.0));
            text_pos_y_slider.set_value(segment.pos_y);
            text_pos_y_value.set_label(&format!("{:.0}%", segment.pos_y * 100.0));
            text_size_slider.set_value(segment.size);
            text_size_value.set_label(&format!("{:.0}%", segment.size * 100.0));
            for (animation, button) in &text_anim_buttons {
                button.set_active(*animation == segment.animation);
            }
            for (scope, button) in &text_scope_buttons {
                button.set_active(*scope == segment.scope);
            }
            preview.set_tooltip_text(Some(&t("Drag on the preview to place the title")));
        } else {
            preview.set_tooltip_text(None);
        }
        if let Some(segment) = selected {
            intensity_slider.set_value(segment.intensity);
            intensity_value.set_label(&format!("{:.0}%", segment.intensity * 100.0));
            zoom_anchor_x_slider.set_value(segment.zoom_anchor_x);
            zoom_anchor_x_value.set_label(&format!("{:.0}%", segment.zoom_anchor_x * 100.0));
            zoom_anchor_y_slider.set_value(segment.zoom_anchor_y);
            zoom_anchor_y_value.set_label(&format!("{:.0}%", segment.zoom_anchor_y * 100.0));
            yaw_slider.set_value(segment.to.rotation_y);
            yaw_value.set_label(&format!("{:.0}°", segment.to.rotation_y));
            pitch_slider.set_value(segment.to.rotation_x);
            pitch_value.set_label(&format!("{:.0}°", segment.to.rotation_x));
            roll_slider.set_value(segment.to.rotation_z);
            roll_value.set_label(&format!("{:.0}°", segment.to.rotation_z));
            perspective_slider.set_value(perspective_intensity);
            perspective_value.set_label(&format!("{:.0}%", perspective_intensity * 100.0));
            position_pad.set_position(segment.to.pos_x, segment.to.pos_y);
            pos_x_slider.set_value(segment.to.pos_x);
            pos_x_value.set_label(&format!("{:.0}", segment.to.pos_x * 1000.0));
            pos_y_slider.set_value(segment.to.pos_y);
            pos_y_value.set_label(&format!("{:.0}", segment.to.pos_y * 1000.0));
            ease_slider.set_value(transform_timing.transition_duration * 1000.0);
            ease_value.set_label(&format!(
                "{:.0}ms",
                transform_timing.transition_duration * 1000.0
            ));
            scale_slider.set_value(segment.to.scale);
        }
        delete_btn.set_sensitive(has_clip || has_text);
        syncing.set(false);
        preview.queue_draw();
        ruler.queue_draw();
        source_track.queue_draw();
        motion_track.queue_draw();
        text_track.queue_draw();
        playhead_overlay.queue_draw();
    })
}

pub(super) fn install_shared(
    parts: &MotionModeParts,
    session: &MotionSession,
    redraw: Redraw,
    request_live_preview: RequestLivePreview,
) {
    parts.shared.duration_slider.connect_value_changed({
        let session_runtime = session.runtime.clone();
        let duration_value = parts.shared.duration_value.clone();
        let redraw = redraw.clone();
        move |slider| {
            let duration = MotionState::clamp_duration(slider.value());
            session_runtime.borrow_mut().motion.set_duration(duration);
            duration_value.set_label(&format_duration_label(duration));
            redraw();
        }
    });

    parts.shared.blur_slider.connect_value_changed({
        let session = session.runtime.clone();
        let blur_value = parts.shared.blur_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let value = slider.value();
            session.borrow_mut().motion.set_motion_blur(value);
            blur_value.set_label(&format!("{:.0}%", value * 100.0));
            request_live_preview();
        }
    });

    parts.shared.blur_shutter_slider.connect_value_changed({
        let session = session.runtime.clone();
        let value_label = parts.shared.blur_shutter_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let shutter = slider.value().clamp(0.0, 360.0);
            session
                .borrow_mut()
                .motion
                .motion_blur_settings
                .shutter_angle = shutter;
            value_label.set_label(&format!("{shutter:.0}°"));
            request_live_preview();
        }
    });

    parts.shared.blur_trail_slider.connect_value_changed({
        let session = session.runtime.clone();
        let value_label = parts.shared.blur_trail_value.clone();
        let request_live_preview = request_live_preview.clone();
        let syncing = parts.shared.inspector_syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            let trail = slider.value().clamp(0.0, 1.0);
            session
                .borrow_mut()
                .motion
                .motion_blur_settings
                .transform_trail_opacity = trail;
            value_label.set_label(&format!("{:.0}%", trail * 100.0));
            request_live_preview();
        }
    });
}
