use gtk4::{gdk, glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{image_transform::StaticImageTransform, state::EditorState};
use crate::i18n::t;
use crate::recording::editor::window::tool_sidebar::FillSlider;

use super::motion_mode::{
    angle_slider_row, motion_settings_section, perspective_slider_row, position_slider_row,
    MotionPositionPad,
};

#[derive(Clone)]
struct TransformControls {
    yaw: FillSlider,
    pitch: FillSlider,
    roll: FillSlider,
    perspective: FillSlider,
    position_pad: MotionPositionPad,
    pos_x: FillSlider,
    pos_y: FillSlider,
    reset: Button,
}

pub(super) fn build_static_transform_panel(
    state: &Arc<Mutex<EditorState>>,
    preview: &gtk4::DrawingArea,
    on_interact: Option<Rc<dyn Fn()>>,
    sync_slot: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> GtkBox {
    build_static_transform_panel_with_controls(state, preview, on_interact, sync_slot).0
}

fn build_static_transform_panel_with_controls(
    state: &Arc<Mutex<EditorState>>,
    preview: &gtk4::DrawingArea,
    on_interact: Option<Rc<dyn Fn()>>,
    sync_slot: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> (GtkBox, TransformControls) {
    let initial = state.lock().unwrap().static_image_transform.sanitized();
    let on_interact = on_interact.unwrap_or_else(|| Rc::new(|| {}));
    let body = GtkBox::new(Orientation::Vertical, 8);

    let studio_hint = Label::new(Some(&t(
        "Studio preview. A non-default rendered pose needs activation for final images; backgrounds and normal edits stay free.",
    )));
    studio_hint.set_visible(!crate::studio_entitlement::studio_unlocked());
    studio_hint.add_css_class("editor-transform-hint");
    studio_hint.set_xalign(0.0);
    studio_hint.set_wrap(true);
    body.append(&studio_hint);

    let rotation_section = motion_settings_section("Rotation & Perspective");
    let (yaw_header, yaw_value, yaw) = angle_slider_row(&t("Yaw"), initial.yaw);
    let (pitch_header, pitch_value, pitch) = angle_slider_row(&t("Pitch"), initial.pitch);
    let (roll_header, roll_value, roll) = angle_slider_row(&t("Roll"), initial.roll);
    let (perspective_value, perspective) = perspective_slider_row(initial.perspective);
    rotation_section.append(&yaw_header);
    rotation_section.append(&yaw.widget());
    rotation_section.append(&pitch_header);
    rotation_section.append(&pitch.widget());
    rotation_section.append(&roll_header);
    rotation_section.append(&roll.widget());
    rotation_section.append(&perspective_value);
    rotation_section.append(&perspective.widget());
    let perspective_hint = Label::new(Some(&t(
        "Tilt the image with Yaw or Pitch before adjusting perspective.",
    )));
    perspective_hint.add_css_class("editor-transform-hint");
    perspective_hint.set_wrap(true);
    perspective_hint.set_xalign(0.0);
    let initially_tilted = initial.yaw.abs() > 1e-7 || initial.pitch.abs() > 1e-7;
    perspective.set_sensitive(initially_tilted);
    perspective_hint.set_visible(!initially_tilted);
    rotation_section.append(&perspective_hint);
    body.append(&rotation_section);

    let position_section = motion_settings_section("Position");
    position_section.add_css_class("editor-motion-position-section");
    let position_pad = MotionPositionPad::new();
    position_pad.set_position(initial.pos_x, initial.pos_y);
    position_section.append(&position_pad.widget());
    let (pos_x_header, pos_x_value, pos_x) = position_slider_row(&t("X"), initial.pos_x);
    let (pos_y_header, pos_y_value, pos_y) = position_slider_row(&t("Y"), initial.pos_y);
    position_section.append(&pos_x_header);
    position_section.append(&pos_x.widget());
    position_section.append(&pos_y_header);
    position_section.append(&pos_y.widget());

    let edit_hint = Label::new(Some(&t("The saved camera pose is paused while drawing.")));
    edit_hint.add_css_class("editor-transform-hint");
    edit_hint.set_xalign(0.0);
    edit_hint.set_wrap(true);
    edit_hint.set_visible(!initial.is_identity());
    position_section.append(&edit_hint);

    let reset = Button::with_label(&t("Reset transforms"));
    reset.add_css_class("editor-transform-reset-button");
    reset.set_halign(Align::Fill);
    reset.set_sensitive(!initial.is_identity());
    position_section.append(&reset);
    body.append(&position_section);

    let syncing = Rc::new(Cell::new(false));
    let controls = TransformControls {
        yaw: yaw.clone(),
        pitch: pitch.clone(),
        roll: roll.clone(),
        perspective: perspective.clone(),
        position_pad: position_pad.clone(),
        pos_x: pos_x.clone(),
        pos_y: pos_y.clone(),
        reset: reset.clone(),
    };
    let sync_controls: Rc<dyn Fn(StaticImageTransform)> = {
        let syncing = syncing.clone();
        let controls = controls.clone();
        let yaw_value = yaw_value.clone();
        let pitch_value = pitch_value.clone();
        let roll_value = roll_value.clone();
        let perspective_value = perspective_value.clone();
        let pos_x_value = pos_x_value.clone();
        let pos_y_value = pos_y_value.clone();
        let edit_hint = edit_hint.clone();
        let perspective_hint = perspective_hint.clone();
        Rc::new(move |transform| {
            let transform = transform.sanitized();
            syncing.set(true);
            controls.yaw.set_value(transform.yaw);
            controls.pitch.set_value(transform.pitch);
            controls.roll.set_value(transform.roll);
            controls.perspective.set_value(transform.perspective);
            let tilted = transform.yaw.abs() > 1e-7 || transform.pitch.abs() > 1e-7;
            controls.perspective.set_sensitive(tilted);
            perspective_hint.set_visible(!tilted);
            controls
                .position_pad
                .set_position(transform.pos_x, transform.pos_y);
            controls.pos_x.set_value(transform.pos_x);
            controls.pos_y.set_value(transform.pos_y);
            yaw_value.set_label(&format!("{:.0}°", transform.yaw));
            pitch_value.set_label(&format!("{:.0}°", transform.pitch));
            roll_value.set_label(&format!("{:.0}°", transform.roll));
            perspective_value.set_label(&format!("{:.0}%", transform.perspective * 100.0));
            pos_x_value.set_label(&format!("{:.0}", transform.pos_x * 1000.0));
            pos_y_value.set_label(&format!("{:.0}", transform.pos_y * 1000.0));
            controls.reset.set_sensitive(!transform.is_identity());
            edit_hint.set_visible(!transform.is_identity());
            syncing.set(false);
        })
    };

    connect_transform_slider(
        &yaw,
        &yaw_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        -24.0,
        24.0,
        1.0,
        5.0,
        |transform, value| transform.yaw = value,
        |value| format!("{value:.0}°"),
    );
    connect_transform_slider(
        &pitch,
        &pitch_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        -24.0,
        24.0,
        1.0,
        5.0,
        |transform, value| transform.pitch = value,
        |value| format!("{value:.0}°"),
    );
    connect_transform_slider(
        &roll,
        &roll_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        -24.0,
        24.0,
        1.0,
        5.0,
        |transform, value| transform.roll = value,
        |value| format!("{value:.0}°"),
    );
    connect_transform_slider(
        &perspective,
        &perspective_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        0.0,
        1.0,
        0.01,
        0.1,
        |transform, value| transform.perspective = value,
        |value| format!("{:.0}%", value * 100.0),
    );
    connect_transform_slider(
        &pos_x,
        &pos_x_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        -1.0,
        1.0,
        0.001,
        0.1,
        |transform, value| transform.pos_x = value,
        |value| format!("{:.0}", value * 1000.0),
    );
    connect_transform_slider(
        &pos_y,
        &pos_y_value,
        state,
        preview,
        &on_interact,
        &syncing,
        &sync_controls,
        -1.0,
        1.0,
        0.001,
        0.1,
        |transform, value| transform.pos_y = value,
        |value| format!("{:.0}", value * 1000.0),
    );

    position_pad.connect_interaction(
        {
            let state = state.clone();
            move || state.lock().unwrap().begin_history_interaction()
        },
        {
            let state = state.clone();
            move || state.lock().unwrap().finish_history_interaction()
        },
    );
    position_pad.connect_value_changed({
        let state = state.clone();
        let preview = preview.clone();
        let on_interact = on_interact.clone();
        let sync_controls = sync_controls.clone();
        move |x, y| {
            let mut transform = state.lock().unwrap().static_image_transform;
            transform.pos_x = x;
            transform.pos_y = y;
            update_transform(&state, transform, &preview, &on_interact, &sync_controls);
        }
    });

    reset.connect_clicked({
        let state = state.clone();
        let preview = preview.clone();
        let on_interact = on_interact.clone();
        let sync_controls = sync_controls.clone();
        move |_| {
            update_transform(
                &state,
                StaticImageTransform::default(),
                &preview,
                &on_interact,
                &sync_controls,
            );
        }
    });

    *sync_slot.borrow_mut() = Some(Rc::new({
        let state = state.clone();
        let sync_controls = sync_controls.clone();
        move || sync_controls(state.lock().unwrap().static_image_transform)
    }));

    (body, controls)
}

fn connect_transform_slider(
    slider: &FillSlider,
    value_label: &Label,
    state: &Arc<Mutex<EditorState>>,
    preview: &gtk4::DrawingArea,
    on_interact: &Rc<dyn Fn()>,
    syncing: &Rc<Cell<bool>>,
    sync_controls: &Rc<dyn Fn(StaticImageTransform)>,
    min: f64,
    max: f64,
    step: f64,
    page: f64,
    update: impl Fn(&mut StaticImageTransform, f64) + 'static,
    format: impl Fn(f64) -> String + 'static,
) {
    let widget = slider.widget();
    widget.set_focusable(true);
    slider.connect_drag_started({
        let state = state.clone();
        let widget = widget.clone();
        move || {
            widget.grab_focus();
            state.lock().unwrap().begin_history_interaction();
        }
    });
    slider.connect_drag_ended({
        let state = state.clone();
        move || state.lock().unwrap().finish_history_interaction()
    });
    let key_interaction = Rc::new(Cell::new(false));
    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed({
        let state = state.clone();
        let slider = slider.clone();
        let syncing = syncing.clone();
        let key_interaction = key_interaction.clone();
        move |_, key, _, _| {
            if syncing.get() {
                return glib::Propagation::Proceed;
            }
            let current = slider.value();
            let next = match key {
                gdk::Key::Left | gdk::Key::Down => Some(current - step),
                gdk::Key::Right | gdk::Key::Up => Some(current + step),
                gdk::Key::Page_Down => Some(current - page),
                gdk::Key::Page_Up => Some(current + page),
                gdk::Key::Home => Some(min),
                gdk::Key::End => Some(max),
                _ => None,
            };
            let Some(next) = next else {
                return glib::Propagation::Proceed;
            };
            if !key_interaction.replace(true) {
                state.lock().unwrap().begin_history_interaction();
            }
            slider.set_value(next.clamp(min, max));
            glib::Propagation::Stop
        }
    });
    keys.connect_key_released({
        let state = state.clone();
        let key_interaction = key_interaction.clone();
        move |_, key, _, _| {
            if matches!(
                key,
                gdk::Key::Left
                    | gdk::Key::Down
                    | gdk::Key::Right
                    | gdk::Key::Up
                    | gdk::Key::Page_Down
                    | gdk::Key::Page_Up
                    | gdk::Key::Home
                    | gdk::Key::End
            ) && key_interaction.replace(false)
            {
                state.lock().unwrap().finish_history_interaction();
            }
        }
    });
    widget.add_controller(keys);
    slider.connect_value_changed({
        let state = state.clone();
        let preview = preview.clone();
        let on_interact = on_interact.clone();
        let syncing = syncing.clone();
        let sync_controls = sync_controls.clone();
        let value_label = value_label.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            value_label.set_label(&format(slider.value()));
            let mut transform = state.lock().unwrap().static_image_transform;
            update(&mut transform, slider.value());
            update_transform(&state, transform, &preview, &on_interact, &sync_controls);
        }
    });
}

fn update_transform(
    state: &Arc<Mutex<EditorState>>,
    transform: StaticImageTransform,
    preview: &gtk4::DrawingArea,
    on_interact: &Rc<dyn Fn()>,
    sync_controls: &Rc<dyn Fn(StaticImageTransform)>,
) {
    let changed = state.lock().unwrap().set_static_image_transform(transform);
    let current = state.lock().unwrap().static_image_transform;
    sync_controls(current);
    if changed {
        on_interact();
        preview.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_button(widget: &gtk4::Widget, label: &str) -> Option<Button> {
        if let Ok(button) = widget.clone().downcast::<Button>() {
            if button.label().as_deref() == Some(label) {
                return Some(button);
            }
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(button) = find_button(&current, label) {
                return Some(button);
            }
            child = current.next_sibling();
        }
        None
    }

    #[test]
    fn motion_pose_controls_sync_undo_redo_and_reset_without_feedback() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = Arc::new(Mutex::new(EditorState::new(image::RgbaImage::new(20, 12))));
            let preview = gtk4::DrawingArea::new();
            let sync_slot = Rc::new(RefCell::new(None));
            let (panel, controls) =
                build_static_transform_panel_with_controls(&state, &preview, None, &sync_slot);
            let transform = StaticImageTransform {
                yaw: 12.0,
                pitch: -18.0,
                roll: 7.0,
                perspective: 0.41,
                pos_x: -0.375,
                pos_y: 0.275,
            };
            assert!(state.lock().unwrap().set_static_image_transform(transform));
            let sync = sync_slot.borrow().clone().unwrap();
            sync();
            assert_eq!(state.lock().unwrap().static_image_transform, transform);
            assert_eq!(state.lock().unwrap().history_availability(), (true, false));

            assert!(state.lock().unwrap().undo());
            sync();
            assert_eq!(
                state.lock().unwrap().static_image_transform,
                StaticImageTransform::default()
            );
            assert_eq!(state.lock().unwrap().history_availability(), (false, true));

            assert!(state.lock().unwrap().redo());
            sync();
            assert_eq!(state.lock().unwrap().static_image_transform, transform);
            assert_eq!(state.lock().unwrap().history_availability(), (true, false));

            controls.pos_x.set_value(0.5);
            assert_eq!(state.lock().unwrap().static_image_transform.pos_x, 0.5);
            find_button(panel.upcast_ref(), "Reset transforms")
                .expect("reset button")
                .emit_clicked();
            assert_eq!(
                state.lock().unwrap().static_image_transform,
                StaticImageTransform::default()
            );
            assert!(state.lock().unwrap().undo());
            assert_eq!(state.lock().unwrap().static_image_transform.pos_x, 0.5);
        }) else {
            return;
        };
    }
}
