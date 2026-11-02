//! Controls for the selected Motion clip and its composition.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, DrawingArea, Label, Orientation, ToggleButton};

use super::annotate::EditorHost;
use super::sections;
use super::timeline::Timeline;
use super::ui_support::px;
use crate::i18n::t;
use crate::recording::editor::model::{MotionEffectTransformTiming, MotionState, MotionTimingKind};
use crate::video_editor::side_panel::FilledSlider;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Blur,
    Shutter,
    Scale,
    Intensity,
    Yaw,
    Pitch,
    Roll,
    Perspective,
    X,
    Y,
    Duration,
    EaseX1,
    EaseY1,
    EaseX2,
    EaseY2,
    Bounce,
}

impl Field {
    fn value(self, state: &MotionState) -> f64 {
        let segment = state.selected_segment();
        let timing = state.selected_transform_timing();
        match self {
            Self::Blur => state.motion_blur * 100.0,
            Self::Shutter => state.motion_blur_settings.shutter_angle,
            Self::Scale => segment.map_or(2.0, |s| s.to.scale),
            Self::Intensity => segment.map_or(100.0, |s| s.intensity * 100.0),
            Self::Yaw => segment.map_or(0.0, |s| s.to.rotation_y),
            Self::Pitch => segment.map_or(0.0, |s| s.to.rotation_x),
            Self::Roll => segment.map_or(0.0, |s| s.to.rotation_z),
            Self::Perspective => state.perspective_intensity * 100.0,
            Self::X => segment.map_or(0.0, |s| s.to.pos_x * 1000.0),
            Self::Y => segment.map_or(0.0, |s| s.to.pos_y * 1000.0),
            Self::Duration => {
                segment.map_or(timing.transition_duration, |s| {
                    timing.transition_duration.min(s.duration())
                }) * 1000.0
            }
            Self::EaseX1 => timing.easing_x1 * 100.0,
            Self::EaseY1 => timing.easing_y1 * 100.0,
            Self::EaseX2 => timing.easing_x2 * 100.0,
            Self::EaseY2 => timing.easing_y2 * 100.0,
            Self::Bounce => timing.spring_bounce * 100.0,
        }
    }

    fn apply(self, state: &mut MotionState, value: f64) {
        match self {
            Self::Blur => state.set_motion_blur(value / 100.0),
            Self::Shutter => state.motion_blur_settings.shutter_angle = value.clamp(0.0, 360.0),
            Self::Scale => state.set_selected_end_scale(value),
            Self::Intensity => state.set_selected_intensity(value / 100.0),
            Self::Yaw => state.set_selected_end_yaw(value),
            Self::Pitch => state.set_selected_end_pitch(value),
            Self::Roll => state.set_selected_end_roll(value),
            Self::Perspective => state.set_selected_perspective(value / 100.0),
            Self::X => state.set_selected_end_pos_x(value / 1000.0),
            Self::Y => state.set_selected_end_pos_y(value / 1000.0),
            Self::Duration => state.set_selected_transition_ms(value.round() as u32),
            Self::EaseX1 | Self::EaseY1 | Self::EaseX2 | Self::EaseY2 | Self::Bounce => {
                let mut timing = state.selected_transform_timing();
                match self {
                    Self::EaseX1 => timing.easing_x1 = value / 100.0,
                    Self::EaseY1 => timing.easing_y1 = value / 100.0,
                    Self::EaseX2 => timing.easing_x2 = value / 100.0,
                    Self::EaseY2 => timing.easing_y2 = value / 100.0,
                    Self::Bounce => timing.spring_bounce = value / 100.0,
                    _ => unreachable!(),
                }
                state.set_transform_timing(timing);
            }
        }
    }
}

pub(super) struct MotionControls {
    pub page: GtkBox,
    pub duration: FilledSlider,
    clip_box: GtkBox,
    hint: Label,
    sliders: Vec<(Field, FilledSlider)>,
    anchor: MotionPad,
    position: MotionPad,
    ease: ToggleButton,
    spring: ToggleButton,
    custom: ToggleButton,
    easing_rows: GtkBox,
    spring_rows: GtkBox,
    kind: Rc<Cell<MotionTimingKind>>,
}

impl MotionControls {
    pub(super) fn build(scale: f64, host: &Rc<EditorHost>) -> Self {
        let page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
        page.add_css_class("image-editor-inspector-body");
        let studio_hint = Label::new(Some(&t(
            "Studio preview. MP4 export needs activation; local editing and project saves remain free.",
        )));
        studio_hint.add_css_class("image-editor-hint");
        studio_hint.set_wrap(true);
        studio_hint.set_xalign(0.0);
        studio_hint.set_visible(!crate::studio_entitlement::studio_unlocked());
        page.append(&studio_hint);

        let composition = sections::section_card("Composition", scale);
        let duration =
            FilledSlider::new(&t("Duration"), 1.0, 10.0, 6.0, 0.5, 1, scale).with_suffix("s");
        composition.append(&duration.widget());
        page.append(&composition);

        let mut sliders = Vec::new();
        let blur = sections::section_card("Motion Blur", scale);
        add_slider(
            &blur,
            &mut sliders,
            Field::Blur,
            "Motion blur",
            (0.0, 100.0, 0.0, 1.0, 0),
            "%",
            scale,
        );
        add_slider(
            &blur,
            &mut sliders,
            Field::Shutter,
            "Blur shutter",
            (0.0, 360.0, 180.0, 5.0, 0),
            "\u{00b0}",
            scale,
        );
        page.append(&blur);

        let hint = Label::new(Some(&t(
            "Click a Motion clip, or click the track to add a move.",
        )));
        hint.add_css_class("image-editor-hint");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        page.append(&hint);

        let clip_box = GtkBox::new(Orientation::Vertical, px(10.0, scale));
        let title = Label::new(Some(&t("Move")));
        title.add_css_class("image-editor-inspector-title");
        title.set_xalign(0.0);
        clip_box.append(&title);

        let transform = sections::section_card("Transform", scale);
        let anchor = MotionPad::new(scale, Some(host.clone()));
        transform.append(&anchor.area);
        add_slider(
            &transform,
            &mut sliders,
            Field::Scale,
            "Scale",
            (1.0, 4.0, 2.0, 0.1, 1),
            "x",
            scale,
        );
        add_slider(
            &transform,
            &mut sliders,
            Field::Intensity,
            "Intensity",
            (0.0, 100.0, 100.0, 1.0, 0),
            "%",
            scale,
        );
        clip_box.append(&transform);

        let rotation = sections::section_card("Rotation & Perspective", scale);
        for (field, label) in [
            (Field::Yaw, "Yaw"),
            (Field::Pitch, "Pitch"),
            (Field::Roll, "Roll"),
        ] {
            add_slider(
                &rotation,
                &mut sliders,
                field,
                label,
                (-24.0, 24.0, 0.0, 1.0, 0),
                "\u{00b0}",
                scale,
            );
        }
        add_slider(
            &rotation,
            &mut sliders,
            Field::Perspective,
            "Perspective",
            (0.0, 100.0, 18.0, 1.0, 0),
            "%",
            scale,
        );
        clip_box.append(&rotation);

        let position_section = sections::section_card("Position", scale);
        let position = MotionPad::new(scale, None);
        position_section.append(&position.area);
        for (field, label) in [(Field::X, "X"), (Field::Y, "Y")] {
            add_slider(
                &position_section,
                &mut sliders,
                field,
                label,
                (-1000.0, 1000.0, 0.0, 10.0, 0),
                "",
                scale,
            );
        }
        clip_box.append(&position_section);

        let timing = sections::section_card("Timing", scale);
        add_slider(
            &timing,
            &mut sliders,
            Field::Duration,
            "Duration",
            (0.0, 10000.0, 1000.0, 20.0, 0),
            "ms",
            scale,
        );
        let mode_row = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        mode_row.add_css_class("recording-editor-zoom-easing");
        mode_row.set_homogeneous(true);
        let ease = timing_button("Ease", MotionTimingKind::Ease, scale);
        let spring = timing_button("Spring", MotionTimingKind::Spring, scale);
        spring.set_group(Some(&ease));
        let custom = ToggleButton::with_label(&t("Custom"));
        custom.add_css_class("recording-editor-zoom-easing-btn");
        custom.set_has_frame(false);
        mode_row.append(&ease);
        mode_row.append(&spring);
        mode_row.append(&custom);
        timing.append(&mode_row);
        let easing_rows = GtkBox::new(Orientation::Vertical, px(6.0, scale));
        for (field, label, initial) in [
            (Field::EaseX1, "Ease X1", 25.0),
            (Field::EaseY1, "Ease Y1", 100.0),
            (Field::EaseX2, "Ease X2", 50.0),
            (Field::EaseY2, "Ease Y2", 100.0),
        ] {
            add_slider(
                &easing_rows,
                &mut sliders,
                field,
                label,
                (0.0, 100.0, initial, 1.0, 0),
                "%",
                scale,
            );
        }
        timing.append(&easing_rows);
        let spring_rows = GtkBox::new(Orientation::Vertical, 0);
        add_slider(
            &spring_rows,
            &mut sliders,
            Field::Bounce,
            "Bounce",
            (0.0, 50.0, 5.0, 1.0, 0),
            "%",
            scale,
        );
        timing.append(&spring_rows);
        clip_box.append(&timing);
        page.append(&clip_box);

        let kind = Rc::new(Cell::new(MotionTimingKind::Ease));
        custom.connect_toggled({
            let kind = kind.clone();
            let easing_rows = easing_rows.clone();
            let spring_rows = spring_rows.clone();
            move |custom| {
                update_custom_rows(custom.is_active(), kind.get(), &easing_rows, &spring_rows)
            }
        });
        clip_box.set_visible(false);
        update_custom_rows(false, kind.get(), &easing_rows, &spring_rows);
        Self {
            page,
            duration,
            clip_box,
            hint,
            sliders,
            anchor,
            position,
            ease,
            spring,
            custom,
            easing_rows,
            spring_rows,
            kind,
        }
    }

    pub(super) fn bind(&self, timeline: &Rc<Timeline>) {
        self.duration.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );
        for (field, slider) in &self.sliders {
            let field = *field;
            slider.enable_keyboard(if field == Field::Duration {
                100.0
            } else {
                10.0
            });
            slider.on_interaction(
                interaction_callback(timeline, true),
                interaction_callback(timeline, false),
            );
            let timeline = Rc::downgrade(timeline);
            slider.on_change(Rc::new(move |value| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(
                        |state| field.apply(state, value),
                        !matches!(field, Field::Blur | Field::Shutter),
                    );
                }
            }));
        }
        self.anchor.on_change({
            let timeline = Rc::downgrade(timeline);
            Rc::new(move |x, y| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(|state| state.set_selected_zoom_anchor(x, y), true);
                }
            })
        });
        self.anchor.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );
        self.position.on_change({
            let timeline = Rc::downgrade(timeline);
            Rc::new(move |x, y| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(
                        |state| {
                            state.set_selected_end_pos_x(x * 2.0 - 1.0);
                            state.set_selected_end_pos_y(y * 2.0 - 1.0);
                        },
                        true,
                    );
                }
            })
        });
        self.position.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );
        for (button, kind) in [
            (&self.ease, MotionTimingKind::Ease),
            (&self.spring, MotionTimingKind::Spring),
        ] {
            let timeline = Rc::downgrade(timeline);
            let custom = self.custom.clone();
            button.connect_clicked(move |_| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(
                        |state| {
                            let mut timing = state.selected_transform_timing();
                            timing.kind = kind;
                            match kind {
                                MotionTimingKind::Ease => {
                                    timing.easing_x1 = 0.42;
                                    timing.easing_y1 = 0.0;
                                    timing.easing_x2 = 0.58;
                                    timing.easing_y2 = 1.0;
                                }
                                MotionTimingKind::Spring => timing.spring_bounce = 0.05,
                            }
                            state.set_transform_timing(timing);
                        },
                        true,
                    );
                    custom.set_active(false);
                }
            });
        }
    }

    pub(super) fn sync(&self, state: &MotionState) {
        let selected = state.selected_segment();
        self.clip_box.set_visible(selected.is_some());
        self.hint
            .set_visible(selected.is_none() && state.selected_text.is_none());
        self.duration.set_value(state.duration);
        for (field, slider) in &self.sliders {
            if *field == Field::Duration {
                let max = selected.map_or(10000.0, |segment| segment.duration() * 1000.0);
                slider.set_range(0.0, max);
            }
            slider.set_value(field.value(state));
        }
        if let Some(segment) = selected {
            self.anchor
                .set_point(segment.zoom_anchor_x, segment.zoom_anchor_y);
            self.position.set_point(
                (segment.to.pos_x + 1.0) / 2.0,
                (segment.to.pos_y + 1.0) / 2.0,
            );
        }
        let kind = state.selected_transform_timing().kind;
        self.kind.set(kind);
        self.ease.set_active(kind == MotionTimingKind::Ease);
        self.spring.set_active(kind == MotionTimingKind::Spring);
        update_custom_rows(
            self.custom.is_active(),
            kind,
            &self.easing_rows,
            &self.spring_rows,
        );
    }

    pub(super) fn refresh_thumbnail(&self) {
        self.anchor.area.queue_draw();
    }
}

fn interaction_callback(timeline: &Rc<Timeline>, start: bool) -> Rc<dyn Fn()> {
    let timeline = Rc::downgrade(timeline);
    Rc::new(move || {
        if let Some(timeline) = timeline.upgrade() {
            if start {
                timeline.begin_edit();
            } else {
                timeline.finish_edit();
            }
        }
    })
}

type SliderSpec = (f64, f64, f64, f64, usize);

fn add_slider(
    parent: &GtkBox,
    sliders: &mut Vec<(Field, FilledSlider)>,
    field: Field,
    label: &str,
    spec: SliderSpec,
    suffix: &'static str,
    scale: f64,
) {
    let (min, max, value, step, decimals) = spec;
    let slider =
        FilledSlider::new(&t(label), min, max, value, step, decimals, scale).with_suffix(suffix);
    parent.append(&slider.widget());
    sliders.push((field, slider));
}

fn timing_button(label: &str, kind: MotionTimingKind, scale: f64) -> ToggleButton {
    let button = ToggleButton::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-zoom-easing-btn");
    let row = GtkBox::new(Orientation::Horizontal, px(4.0, scale));
    row.set_halign(gtk4::Align::Center);
    let curve = DrawingArea::new();
    curve.set_size_request(px(22.0, scale), px(18.0, scale));
    curve.set_can_target(false);
    curve.set_draw_func(move |_, context, width, height| {
        let timing = MotionEffectTransformTiming {
            kind,
            easing_x1: 0.42,
            easing_y1: 0.0,
            easing_x2: 0.58,
            easing_y2: 1.0,
            spring_bounce: 0.05,
            ..Default::default()
        };
        let (width, height) = (f64::from(width), f64::from(height));
        context.set_source_rgba(0.88, 0.55, 0.37, 0.95);
        context.set_line_width(1.5 * scale);
        for step in 0..=32 {
            let fraction = f64::from(step) / 32.0;
            let x = 1.0 + fraction * (width - 2.0).max(1.0);
            let y =
                height - 2.0 - timing.apply(fraction).clamp(0.0, 1.15) * (height - 4.0).max(1.0);
            if step == 0 {
                context.move_to(x, y);
            } else {
                context.line_to(x, y);
            }
        }
        context.stroke().ok();
    });
    row.append(&curve);
    row.append(&Label::new(Some(&t(label))));
    button.set_child(Some(&row));
    button
}

fn update_custom_rows(custom: bool, kind: MotionTimingKind, ease: &GtkBox, spring: &GtkBox) {
    ease.set_visible(custom && kind == MotionTimingKind::Ease);
    spring.set_visible(custom && kind == MotionTimingKind::Spring);
}

type PadListener = Rc<dyn Fn(f64, f64)>;
type InteractionListener = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

struct MotionPad {
    area: DrawingArea,
    point: Rc<Cell<(f64, f64)>>,
    listener: Rc<RefCell<Option<PadListener>>>,
    on_start: InteractionListener,
    on_end: InteractionListener,
}

impl MotionPad {
    fn new(scale: f64, host: Option<Rc<EditorHost>>) -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, px(152.0, scale));
        area.set_hexpand(true);
        area.add_css_class("image-editor-position-pad");
        area.set_cursor_from_name(Some("crosshair"));
        area.set_tooltip_text(Some(&t(if host.is_some() {
            "Click or drag to place the zoom anchor; double-click to recenter"
        } else {
            "Click or drag to position the camera; double-click to recenter"
        })));
        let point = Rc::new(Cell::new((0.5, 0.5)));
        let listener = Rc::new(RefCell::new(None::<PadListener>));
        let on_start = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));
        let on_end = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));
        let active = Rc::new(Cell::new(false));
        let start: Rc<dyn Fn()> = {
            let active = active.clone();
            let on_start = on_start.clone();
            Rc::new(move || {
                if !active.replace(true) {
                    if let Some(callback) = on_start.borrow().as_ref() {
                        callback();
                    }
                }
            })
        };
        let end: Rc<dyn Fn()> = {
            let active = active.clone();
            let on_end = on_end.clone();
            Rc::new(move || {
                if active.replace(false) {
                    if let Some(callback) = on_end.borrow().as_ref() {
                        callback();
                    }
                }
            })
        };
        area.set_draw_func({
            let point = point.clone();
            let host = host.clone();
            move |area, context, width, height| {
                draw_pad(
                    area,
                    context,
                    f64::from(width),
                    f64::from(height),
                    scale,
                    point.get(),
                    host.as_deref(),
                );
            }
        });
        let apply: Rc<dyn Fn(f64, f64, bool)> = {
            let area = area.downgrade();
            let point = point.clone();
            let listener = listener.clone();
            let host = host.clone();
            Rc::new(move |x, y, reset| {
                let Some(area) = area.upgrade() else { return };
                let (left, top, width, height) = pad_rect(
                    f64::from(area.width().max(1)),
                    f64::from(area.height().max(1)),
                    scale,
                    host.as_deref(),
                );
                let value = if reset {
                    (0.5, 0.5)
                } else {
                    (
                        ((x - left) / width).clamp(0.0, 1.0),
                        ((y - top) / height).clamp(0.0, 1.0),
                    )
                };
                point.set(value);
                area.queue_draw();
                if let Some(callback) = listener.borrow().as_ref() {
                    callback(value.0, value.1);
                }
            })
        };
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let apply = apply.clone();
            let start = start.clone();
            move |_, count, x, y| {
                start();
                apply(x, y, count >= 2);
            }
        });
        click.connect_released({
            let end = end.clone();
            move |_, _, _, _| end()
        });
        area.add_controller(click);
        let drag = gtk4::GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin(move |_, _, _| start());
        drag.connect_drag_update(move |gesture, dx, dy| {
            if let Some((x, y)) = gesture.start_point() {
                apply(x + dx, y + dy, false);
            }
        });
        drag.connect_drag_end(move |_, _, _| end());
        area.add_controller(drag);
        Self {
            area,
            point,
            listener,
            on_start,
            on_end,
        }
    }

    fn set_point(&self, x: f64, y: f64) {
        if self.point.replace((x, y)) != (x, y) {
            self.area.queue_draw();
        }
    }

    fn on_change(&self, listener: PadListener) {
        *self.listener.borrow_mut() = Some(listener);
    }

    fn on_interaction(&self, start: Rc<dyn Fn()>, end: Rc<dyn Fn()>) {
        *self.on_start.borrow_mut() = Some(start);
        *self.on_end.borrow_mut() = Some(end);
    }
}

fn pad_rect(
    width: f64,
    height: f64,
    scale: f64,
    host: Option<&EditorHost>,
) -> (f64, f64, f64, f64) {
    if host.is_none() {
        return (0.0, 0.0, width.max(1.0), height.max(1.0));
    }
    let inset = 6.0 * scale;
    let available_width = (width - inset * 2.0).max(1.0);
    let available_height = (height - inset * 2.0).max(1.0);
    let Some((image_width, image_height)) = host.and_then(|host| {
        host.read(|state| (state.working_image.width(), state.working_image.height()))
    }) else {
        return (0.0, 0.0, width.max(1.0), height.max(1.0));
    };
    let image_width = f64::from(image_width.max(1));
    let image_height = f64::from(image_height.max(1));
    let fit = (available_width / image_width).min(available_height / image_height);
    let (image_width, image_height) = (image_width * fit, image_height * fit);
    (
        (width - image_width) / 2.0,
        (height - image_height) / 2.0,
        image_width,
        image_height,
    )
}

fn draw_pad(
    area: &DrawingArea,
    context: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    scale: f64,
    point: (f64, f64),
    host: Option<&EditorHost>,
) {
    if host.is_none() {
        sections::paint_position_pad(
            area,
            context,
            width as i32,
            height as i32,
            &(point.0 * 2.0 - 1.0, point.1 * 2.0 - 1.0),
        );
        return;
    }
    crate::capture::editor::render::rounded_rect_path(
        context,
        0.0,
        0.0,
        width,
        height,
        10.0 * scale,
    );
    if sections::widget_is_light(area) {
        context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
    } else {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    }
    context.fill().ok();
    let (x, y, w, h) = pad_rect(width, height, scale, host);
    if let Some(surface) = host.and_then(EditorHost::motion_thumbnail) {
        let _ = context.save();
        crate::capture::editor::render::rounded_rect_path(context, x, y, w, h, 8.0 * scale);
        context.clip();
        context.translate(x, y);
        context.scale(
            w / f64::from(surface.width()),
            h / f64::from(surface.height()),
        );
        context.set_source_surface(&surface, 0.0, 0.0).ok();
        context.paint().ok();
        let _ = context.restore();
    }
    let _ = context.save();
    context.rectangle(x, y, w, h);
    context.clip();
    context.set_source_rgba(1.0, 1.0, 1.0, 0.28);
    context.set_line_width(scale);
    for third in 1..3 {
        let fraction = f64::from(third) / 3.0;
        context.move_to(x + w * fraction, y);
        context.line_to(x + w * fraction, y + h);
        context.move_to(x, y + h * fraction);
        context.line_to(x + w, y + h * fraction);
        context.stroke().ok();
    }
    let _ = context.restore();
    let (cx, cy) = (x + point.0 * w, y + point.1 * h);
    sections::paint_soft_handle(context, cx, cy);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_edit_selected_clip_and_undo_a_slider_drag() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let timeline = Rc::new(super::super::timeline::build(1.0, true));
            let controls = Rc::new(MotionControls::build(1.0, &host));
            controls.bind(&timeline);
            timeline.set_state_listener({
                let timeline = Rc::downgrade(&timeline);
                let controls = Rc::downgrade(&controls);
                Rc::new(move || {
                    if let (Some(timeline), Some(controls)) =
                        (timeline.upgrade(), controls.upgrade())
                    {
                        controls.sync(&timeline.motion_state());
                    }
                })
            });
            assert!(!controls.clip_box.is_visible());
            timeline.edit_motion(
                |state| {
                    state.add_segment_at(0.0);
                },
                true,
            );
            assert!(controls.clip_box.is_visible());
            let slider = &controls
                .sliders
                .iter()
                .find(|(field, _)| *field == Field::Scale)
                .unwrap()
                .1;
            let area = slider.widget();
            area.measure(Orientation::Horizontal, -1);
            let height = area.measure(Orientation::Vertical, 240).0.max(32);
            area.allocate(240, height, -1, None);
            let controllers = area.observe_controllers();
            let drag = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .find_map(|item| item.downcast::<gtk4::GestureDrag>().ok())
                .unwrap();
            drag.emit_by_name::<()>("drag-begin", &[&192.0_f64, &10.0_f64]);
            drag.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
            let state = timeline.motion_state();
            assert!((state.selected_segment().unwrap().to.scale - 3.4).abs() < 0.001);

            let toolbar = timeline.dock.first_child().unwrap().first_child().unwrap();
            let right = toolbar.last_child().unwrap();
            let undo = right
                .first_child()
                .unwrap()
                .downcast::<gtk4::Button>()
                .unwrap();
            let redo = undo
                .next_sibling()
                .unwrap()
                .downcast::<gtk4::Button>()
                .unwrap();
            assert!(undo.is_sensitive());
            undo.emit_clicked();
            assert_eq!(
                timeline.motion_state().selected_segment().unwrap().to.scale,
                2.0
            );
            assert!(redo.is_sensitive());
            redo.emit_clicked();
            assert!(
                (timeline.motion_state().selected_segment().unwrap().to.scale - 3.4).abs() < 0.001
            );

            controls.spring.emit_clicked();
            assert_eq!(
                timeline.motion_state().selected_transform_timing().kind,
                MotionTimingKind::Spring
            );
            controls.custom.set_active(true);
            assert!(controls.spring_rows.is_visible());
            assert!(!controls.easing_rows.is_visible());
            timeline.edit_motion(
                |state| {
                    state.add_segment_at(2.0);
                    state.set_selected_end_yaw(12.0);
                },
                true,
            );
            let state = timeline.motion_state();
            assert_eq!(state.segments[0].to.rotation_y, 0.0);
            assert_eq!(state.segments[1].to.rotation_y, 12.0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn motion_fields_use_the_shared_model_and_anchor_mapping() {
        let mut state = MotionState::default();
        state.add_segment_at(1.0);
        for (field, value) in [
            (Field::Yaw, 20.0),
            (Field::Pitch, -12.0),
            (Field::Roll, 5.0),
            (Field::Perspective, 60.0),
            (Field::X, 250.0),
            (Field::Y, -500.0),
            (Field::Intensity, 75.0),
            (Field::Duration, 400.0),
            (Field::EaseX1, 40.0),
        ] {
            field.apply(&mut state, value);
            assert!((field.value(&state) - value).abs() < 1e-6);
        }
        state.set_selected_zoom_anchor(0.2, 0.8);
        let at_end = state.sample(2.0);
        assert_eq!(at_end.rotation_y, 15.0);
        assert_eq!(at_end.pos_x, 0.1875);
        assert_eq!(state.zoom_anchor_at(2.0), (0.2, 0.8));
    }

    #[test]
    fn anchor_pad_uses_the_image_aspect_ratio() {
        let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
        *host.editor.borrow_mut() = Some(crate::capture::editor::state::EditorState::new(
            image::RgbaImage::new(100, 80),
        ));
        let rect = pad_rect(200.0, 100.0, 1.0, Some(&host));
        for (actual, expected) in [
            (rect.0, 45.0),
            (rect.1, 6.0),
            (rect.2, 110.0),
            (rect.3, 88.0),
        ] {
            assert!((actual - expected).abs() < 1e-9);
        }
        assert!(host.motion_thumbnail().is_some());
        assert!(host.motion_thumbnail().is_some());
        assert_eq!(pad_rect(200.0, 100.0, 1.0, None), (0.0, 0.0, 200.0, 100.0));
    }
}
