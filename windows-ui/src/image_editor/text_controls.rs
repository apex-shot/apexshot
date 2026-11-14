//! The Motion Text tool page and its model wiring.
//!
//! The page is the Linux Text inspector in the Windows dock's language:
//! Content, Typography, Placement and Animation cards, with an empty state
//! that offers the timeline's add action. Every control edits the selected
//! `MotionTextSegment` through the timeline, so the preview and the panel
//! always read the same state.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, DropDown, EventControllerFocus, EventControllerKey,
    GestureClick, GestureDrag, Grid, Image, Label, Orientation, Popover, PositionType,
    ScrolledWindow, TextView, ToggleButton,
};

use super::sections;
use super::timeline::Timeline;
use super::ui_support::px;
use crate::i18n::t;
use crate::recording::editor::model::{
    MotionState, MotionTextAlignment, MotionTextAnimation, MotionTextCoordinateSpace,
    MotionTextScope, DEFAULT_MOTION_TEXT_LINE_SPACING, DEFAULT_MOTION_TEXT_POS_X,
    DEFAULT_MOTION_TEXT_POS_Y, DEFAULT_MOTION_TEXT_SIZE, DEFAULT_MOTION_TEXT_TRANSITION_SECONDS,
    DEFAULT_MOTION_TEXT_TYPEWRITER_SECONDS, MAX_MOTION_TEXT_LETTER_SPACING,
    MAX_MOTION_TEXT_LINE_SPACING, MAX_MOTION_TEXT_OUTLINE, MAX_MOTION_TEXT_POS,
    MAX_MOTION_TEXT_ROTATION, MAX_MOTION_TEXT_SIZE, MAX_MOTION_TEXT_TRANSITION_SECONDS,
    MIN_MOTION_TEXT_CANVAS_POS, MIN_MOTION_TEXT_LETTER_SPACING, MIN_MOTION_TEXT_LINE_SPACING,
    MIN_MOTION_TEXT_POS, MIN_MOTION_TEXT_ROTATION, MIN_MOTION_TEXT_SIZE,
    MIN_MOTION_TEXT_TRANSITION_SECONDS,
};
use crate::video_editor::custom_fill::{build_color_picker, ColorPicker};
use crate::video_editor::side_panel::FilledSlider;

pub(super) struct TextControls {
    pub page: GtkBox,
    empty: GtkBox,
    add: Button,
    editor: GtkBox,
    text_view: TextView,
    font: DropDown,
    families: Rc<Vec<String>>,
    bold: ToggleButton,
    italic: ToggleButton,
    shadow: ToggleButton,
    color: TextColorControl,
    opacity: FilledSlider,
    alignment: Vec<(MotionTextAlignment, ToggleButton)>,
    width: FilledSlider,
    size: FilledSlider,
    line_spacing: FilledSlider,
    letter_spacing: FilledSlider,
    rotation: FilledSlider,
    outline: FilledSlider,
    attachment: Vec<(MotionTextCoordinateSpace, ToggleButton)>,
    pad: TextPad,
    readout: Label,
    animations: Vec<(MotionTextAnimation, ToggleButton)>,
    scope_box: GtkBox,
    scopes: Vec<(MotionTextScope, ToggleButton)>,
    transition: FilledSlider,
    typewriter: FilledSlider,
    typewriter_box: GtkBox,
    syncing: Rc<Cell<bool>>,
}

impl TextControls {
    pub(super) fn build(scale: f64, column: &GtkBox) -> Self {
        let syncing = Rc::new(Cell::new(false));
        let page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
        page.add_css_class("image-editor-inspector-body");

        let studio_hint = Label::new(Some(&t(
            "Motion text and its animation are Studio previews; final MP4 export needs activation.",
        )));
        studio_hint.add_css_class("image-editor-hint");
        studio_hint.set_wrap(true);
        studio_hint.set_xalign(0.0);
        studio_hint.set_visible(!crate::studio_entitlement::studio_unlocked());
        page.append(&studio_hint);

        let empty = GtkBox::new(Orientation::Vertical, px(8.0, scale));
        let empty_hint = Label::new(Some(&t("Select a text clip, or add one at the playhead")));
        empty_hint.add_css_class("image-editor-hint");
        empty_hint.set_wrap(true);
        empty_hint.set_xalign(0.0);
        empty_hint.set_max_width_chars(28);
        empty.append(&empty_hint);
        let add = Button::with_label(&t("Add Text"));
        add.add_css_class("image-editor-primary-button");
        add.set_halign(Align::Start);
        empty.append(&add);
        page.append(&empty);

        let editor = GtkBox::new(Orientation::Vertical, px(10.0, scale));
        let content = sections::section_card("Content", scale);
        let text_view = TextView::new();
        text_view.add_css_class("image-editor-text-content");
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.set_accepts_tab(false);
        text_view.set_left_margin(px(10.0, scale));
        text_view.set_right_margin(px(10.0, scale));
        text_view.set_top_margin(px(8.0, scale));
        text_view.set_bottom_margin(px(8.0, scale));
        text_view.buffer().set_enable_undo(true);
        text_view.set_tooltip_text(Some(&t("Type your text; press Enter for a new line")));
        let text_scroll = ScrolledWindow::new();
        text_scroll.add_css_class("image-editor-text-scroll");
        text_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        text_scroll.set_size_request(-1, px(88.0, scale));
        text_scroll.set_child(Some(&text_view));
        content.append(&text_scroll);
        editor.append(&content);

        let typography = sections::section_card("Typography", scale);
        let families = Rc::new(font_families());
        let family_refs: Vec<&str> = families.iter().map(String::as_str).collect();
        let font = DropDown::from_strings(&family_refs);
        font.add_css_class("image-editor-font-dropdown");
        font.set_hexpand(true);
        font.set_enable_search(true);
        font.set_tooltip_text(Some(&t("Font family")));
        typography.append(&font);

        let style_row = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        style_row.add_css_class("recording-editor-zoom-easing");
        style_row.set_homogeneous(true);
        let bold = toggle_button("Bold", scale);
        let italic = toggle_button("Italic", scale);
        style_row.append(&bold);
        style_row.append(&italic);
        typography.append(&style_row);

        let color = TextColorControl::new(scale, column);
        typography.append(&color.row);

        let opacity = slider("Opacity", (0.0, 100.0, 100.0, 1.0, 0), "%", scale);
        typography.append(&opacity.widget());

        let alignment: Vec<(MotionTextAlignment, ToggleButton)> = MotionTextAlignment::ALL
            .iter()
            .map(|&value| (value, toggle_button(value.label(), scale)))
            .collect();
        typography.append(&toggle_row(&alignment));
        group_toggles(&alignment.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>());

        let width = FilledSlider::new(&t("Width"), 0.0, 1.0, 0.0, 0.01, 2, scale);
        width.widget().set_tooltip_text(Some(&t(
            "Paragraph width; Auto keeps the natural line width",
        )));
        typography.append(&width.widget());

        let size = slider(
            "Size",
            (
                MIN_MOTION_TEXT_SIZE,
                MAX_MOTION_TEXT_SIZE,
                DEFAULT_MOTION_TEXT_SIZE,
                0.05,
                2,
            ),
            "x",
            scale,
        );
        typography.append(&size.widget());

        let line_spacing = slider(
            "Line spacing",
            (
                MIN_MOTION_TEXT_LINE_SPACING,
                MAX_MOTION_TEXT_LINE_SPACING,
                DEFAULT_MOTION_TEXT_LINE_SPACING,
                0.01,
                2,
            ),
            "x",
            scale,
        );
        typography.append(&line_spacing.widget());

        let letter_spacing = slider(
            "Letter spacing",
            (
                MIN_MOTION_TEXT_LETTER_SPACING,
                MAX_MOTION_TEXT_LETTER_SPACING,
                0.0,
                0.01,
                2,
            ),
            "em",
            scale,
        );
        typography.append(&letter_spacing.widget());

        let rotation = slider(
            "Rotation",
            (
                MIN_MOTION_TEXT_ROTATION,
                MAX_MOTION_TEXT_ROTATION,
                0.0,
                1.0,
                0,
            ),
            "\u{00b0}",
            scale,
        );
        typography.append(&rotation.widget());

        let outline = slider(
            "Outline",
            (0.0, MAX_MOTION_TEXT_OUTLINE, 0.0, 0.005, 2),
            "em",
            scale,
        );
        typography.append(&outline.widget());

        let shadow = toggle_button("Shadow", scale);
        typography.append(&shadow);
        editor.append(&typography);

        let placement = sections::section_card("Placement", scale);
        let attachment: Vec<(MotionTextCoordinateSpace, ToggleButton)> = [
            (MotionTextCoordinateSpace::Canvas, t("Canvas")),
            (MotionTextCoordinateSpace::image(), t("Image")),
        ]
        .iter()
        .map(|(space, label)| (*space, toggle_button(label, scale)))
        .collect();
        placement.append(&toggle_row(&attachment));
        group_toggles(
            &attachment
                .iter()
                .map(|(_, b)| b.clone())
                .collect::<Vec<_>>(),
        );
        let pad = TextPad::new(scale);
        placement.append(&pad.area);
        let readout = Label::new(Some(&position_readout(
            DEFAULT_MOTION_TEXT_POS_X,
            DEFAULT_MOTION_TEXT_POS_Y,
        )));
        readout.add_css_class("image-editor-hint");
        readout.set_xalign(0.0);
        placement.append(&readout);
        editor.append(&placement);

        let animation = sections::section_card("Animation", scale);
        let anim_grid = Grid::new();
        anim_grid.add_css_class("recording-editor-zoom-easing");
        anim_grid.set_hexpand(true);
        anim_grid.set_column_spacing(px(6.0, scale) as u32);
        anim_grid.set_row_spacing(px(6.0, scale) as u32);
        anim_grid.set_column_homogeneous(true);
        let animations: Vec<(MotionTextAnimation, ToggleButton)> = MotionTextAnimation::ALL
            .iter()
            .enumerate()
            .map(|(index, &value)| {
                let button = toggle_button(value.label(), scale);
                anim_grid.attach(&button, (index % 3) as i32, (index / 3) as i32, 1, 1);
                (value, button)
            })
            .collect();
        group_toggles(
            &animations
                .iter()
                .map(|(_, b)| b.clone())
                .collect::<Vec<_>>(),
        );
        animation.append(&anim_grid);

        let scope_box = GtkBox::new(Orientation::Vertical, px(6.0, scale));
        scope_box.set_tooltip_text(Some(&t("Choose how Typewriter reveals the text")));
        let scope_label = Label::new(Some(&t("Scope")));
        scope_label.add_css_class("image-editor-hint");
        scope_label.set_xalign(0.0);
        scope_box.append(&scope_label);
        let scopes: Vec<(MotionTextScope, ToggleButton)> = MotionTextScope::ALL
            .iter()
            .map(|&value| (value, toggle_button(value.label(), scale)))
            .collect();
        scope_box.append(&toggle_row(&scopes));
        group_toggles(&scopes.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>());
        scope_box.set_visible(false);
        animation.append(&scope_box);

        let transition = FilledSlider::new(
            &t("Duration"),
            MIN_MOTION_TEXT_TRANSITION_SECONDS * 1000.0,
            MAX_MOTION_TEXT_TRANSITION_SECONDS * 1000.0,
            DEFAULT_MOTION_TEXT_TRANSITION_SECONDS * 1000.0,
            10.0,
            0,
            scale,
        )
        .with_suffix("ms");
        transition
            .widget()
            .set_tooltip_text(Some(&t("Entrance duration for slides and fades")));
        transition.widget().set_visible(false);
        animation.append(&transition.widget());

        let typewriter_box = GtkBox::new(Orientation::Vertical, 0);
        let typewriter = FilledSlider::new(
            &t("Duration"),
            MIN_MOTION_TEXT_TRANSITION_SECONDS * 1000.0,
            MAX_MOTION_TEXT_TRANSITION_SECONDS * 1000.0,
            DEFAULT_MOTION_TEXT_TYPEWRITER_SECONDS * 1000.0,
            10.0,
            0,
            scale,
        )
        .with_suffix("ms");
        typewriter
            .widget()
            .set_tooltip_text(Some(&t("Time to reveal all text")));
        typewriter_box.append(&typewriter.widget());
        typewriter_box.set_visible(false);
        animation.append(&typewriter_box);
        editor.append(&animation);

        editor.set_visible(false);
        page.append(&editor);

        Self {
            page,
            empty,
            add,
            editor,
            text_view,
            font,
            families,
            bold,
            italic,
            shadow,
            color,
            opacity,
            alignment,
            width,
            size,
            line_spacing,
            letter_spacing,
            rotation,
            outline,
            attachment,
            pad,
            readout,
            animations,
            scope_box,
            scopes,
            transition,
            typewriter,
            typewriter_box,
            syncing,
        }
    }

    pub(super) fn bind(&self, timeline: &Rc<Timeline>) {
        let syncing = self.syncing.clone();

        self.add.connect_clicked({
            let timeline = Rc::downgrade(timeline);
            move |_| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(
                        |state| {
                            let start = state.playhead;
                            state.add_text_at(start);
                        },
                        false,
                    );
                }
            }
        });

        let typing = Rc::new(Cell::new(false));
        self.text_view.buffer().connect_changed({
            let timeline = Rc::downgrade(timeline);
            let syncing = syncing.clone();
            let typing = typing.clone();
            move |buffer| {
                if syncing.get() {
                    return;
                }
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                if let Some(timeline) = timeline.upgrade() {
                    // Typing is one undo step: the edit opens on the first
                    // change and closes on focus-out or Enter, so the history
                    // does not walk back through every keystroke.
                    if !typing.replace(true) {
                        timeline.begin_edit();
                    }
                    timeline.edit_motion(|state| state.set_selected_text_value(text), false);
                }
            }
        });
        let focus = EventControllerFocus::new();
        focus.connect_leave({
            let timeline = Rc::downgrade(timeline);
            let typing = typing.clone();
            move |_| {
                if typing.replace(false) {
                    if let Some(timeline) = timeline.upgrade() {
                        timeline.finish_edit();
                    }
                }
            }
        });
        self.text_view.add_controller(focus);
        let keys = EventControllerKey::new();
        keys.connect_key_pressed({
            let timeline = Rc::downgrade(timeline);
            let typing = typing.clone();
            move |_, key, _, _| {
                if (key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::KP_Enter)
                    && typing.replace(false)
                {
                    if let Some(timeline) = timeline.upgrade() {
                        timeline.finish_edit();
                    }
                }
                gtk4::glib::Propagation::Proceed
            }
        });
        self.text_view.add_controller(keys);

        self.font.connect_selected_notify({
            let timeline = Rc::downgrade(timeline);
            let syncing = syncing.clone();
            let families = self.families.clone();
            move |dropdown| {
                if syncing.get() {
                    return;
                }
                let Some(family) = families.get(dropdown.selected() as usize).cloned() else {
                    return;
                };
                if let Some(timeline) = timeline.upgrade() {
                    timeline
                        .edit_motion(|state| state.set_selected_text_font_family(family), false);
                }
            }
        });

        connect_toggle(&self.bold, &syncing, timeline, |state, active| {
            state.set_selected_text_bold(active)
        });
        connect_toggle(&self.italic, &syncing, timeline, |state, active| {
            state.set_selected_text_italic(active)
        });
        connect_toggle(&self.shadow, &syncing, timeline, |state, active| {
            state.set_selected_text_shadow(active)
        });

        self.color.connect_changed({
            let timeline = Rc::downgrade(timeline);
            move |color| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(|state| state.set_selected_text_color(color), false);
                }
            }
        });
        // The picker's open and close bracket one undo step for the whole
        // drag, so releasing it does not leave every sampled color behind.
        self.color.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );

        connect_slider(&self.opacity, &syncing, timeline, |state, value| {
            state.set_selected_text_opacity(value / 100.0)
        });
        connect_slider(&self.width, &syncing, timeline, |state, value| {
            state.set_selected_text_wrap_width(value)
        });
        connect_slider(&self.size, &syncing, timeline, |state, value| {
            state.set_selected_text_size(value)
        });
        connect_slider(&self.line_spacing, &syncing, timeline, |state, value| {
            state.set_selected_text_line_spacing(value)
        });
        connect_slider(&self.letter_spacing, &syncing, timeline, |state, value| {
            state.set_selected_text_letter_spacing(value)
        });
        connect_slider(&self.rotation, &syncing, timeline, |state, value| {
            state.set_selected_text_rotation(value)
        });
        connect_slider(&self.outline, &syncing, timeline, |state, value| {
            state.set_selected_text_outline_width(value)
        });

        for (alignment, button) in &self.alignment {
            let alignment = *alignment;
            button.connect_toggled({
                let timeline = Rc::downgrade(timeline);
                let syncing = syncing.clone();
                move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    if let Some(timeline) = timeline.upgrade() {
                        timeline.edit_motion(
                            |state| state.set_selected_text_alignment(alignment),
                            false,
                        );
                    }
                }
            });
        }

        for (space, button) in &self.attachment {
            let space = *space;
            button.connect_toggled({
                let timeline = Rc::downgrade(timeline);
                let syncing = syncing.clone();
                move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    let Some(timeline) = timeline.upgrade() else {
                        return;
                    };
                    let position = timeline
                        .motion_state()
                        .selected_text_segment()
                        .map(|segment| (segment.pos_x, segment.pos_y));
                    let Some((pos_x, pos_y)) = position else {
                        return;
                    };
                    timeline.edit_motion(
                        |state| state.set_selected_text_attachment(space, pos_x, pos_y),
                        false,
                    );
                }
            });
        }

        self.pad.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );
        self.pad.connect_value_changed({
            let timeline = Rc::downgrade(timeline);
            let syncing = syncing.clone();
            let readout = self.readout.clone();
            move |x, y| {
                if syncing.get() {
                    return;
                }
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(|state| state.set_selected_text_pos(x, y), false);
                }
                readout.set_text(&position_readout(x, y));
            }
        });

        for (animation, button) in &self.animations {
            let animation = *animation;
            button.connect_toggled({
                let timeline = Rc::downgrade(timeline);
                let syncing = syncing.clone();
                move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    if let Some(timeline) = timeline.upgrade() {
                        timeline.edit_motion(
                            |state| state.set_selected_text_animation(animation),
                            false,
                        );
                    }
                }
            });
        }

        for (scope, button) in &self.scopes {
            let scope = *scope;
            button.connect_toggled({
                let timeline = Rc::downgrade(timeline);
                let syncing = syncing.clone();
                move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    if let Some(timeline) = timeline.upgrade() {
                        timeline.edit_motion(|state| state.set_selected_text_scope(scope), false);
                    }
                }
            });
        }

        connect_slider(&self.transition, &syncing, timeline, |state, value| {
            state.set_selected_text_transition_duration(value / 1000.0)
        });
        connect_slider(&self.typewriter, &syncing, timeline, |state, value| {
            state.set_selected_text_typewriter_time(value / 1000.0)
        });
    }

    pub(super) fn sync(&self, state: &MotionState) {
        let selected = state.selected_text_segment();
        self.empty.set_visible(selected.is_none());
        self.editor.set_visible(selected.is_some());
        let Some(segment) = selected else {
            return;
        };
        self.syncing.set(true);
        let buffer = self.text_view.buffer();
        if buffer.text(&buffer.start_iter(), &buffer.end_iter(), false) != segment.text {
            buffer.set_text(&segment.text);
        }
        if let Some(index) = self
            .families
            .iter()
            .position(|family| family == &segment.format.font_family)
        {
            self.font.set_selected(index as u32);
        }
        self.bold.set_active(segment.format.bold);
        self.italic.set_active(segment.format.italic);
        self.shadow.set_active(segment.format.shadow);
        self.color.set_color(segment.format.color);
        self.opacity.set_value(segment.format.color[3] * 100.0);
        self.width.set_value(segment.format.wrap_width);
        self.size.set_value(segment.size);
        self.line_spacing.set_value(segment.format.line_spacing);
        self.letter_spacing.set_value(segment.format.letter_spacing);
        self.rotation.set_value(segment.format.rotation);
        self.outline.set_value(segment.format.outline_width);
        for (alignment, button) in &self.alignment {
            button.set_active(*alignment == segment.format.alignment);
        }
        let canvas = segment.annotation_coordinate_space.is_canvas();
        for (space, button) in &self.attachment {
            button.set_active(
                *space
                    == if canvas {
                        MotionTextCoordinateSpace::Canvas
                    } else {
                        MotionTextCoordinateSpace::image()
                    },
            );
        }
        self.pad.set_attachment(canvas);
        self.pad.set_position(segment.pos_x, segment.pos_y);
        self.readout
            .set_text(&position_readout(segment.pos_x, segment.pos_y));
        for (animation, button) in &self.animations {
            button.set_active(*animation == segment.animation);
        }
        let typewriter = segment.animation == MotionTextAnimation::Typewriter;
        self.scope_box.set_visible(typewriter);
        for (scope, button) in &self.scopes {
            button.set_active(*scope == segment.scope);
        }
        self.transition.widget().set_visible(matches!(
            segment.animation,
            MotionTextAnimation::SlideFromLeft
                | MotionTextAnimation::SlideFromRight
                | MotionTextAnimation::SlideTop
                | MotionTextAnimation::SlideBottom
                | MotionTextAnimation::Fade
        ));
        self.transition
            .set_value(segment.transition_duration * 1000.0);
        self.typewriter_box.set_visible(typewriter);
        self.typewriter.set_value(segment.typewriter_time * 1000.0);
        self.syncing.set(false);
    }
}

fn connect_toggle(
    button: &ToggleButton,
    syncing: &Rc<Cell<bool>>,
    timeline: &Rc<Timeline>,
    apply: impl Fn(&mut MotionState, bool) + 'static,
) {
    button.connect_toggled({
        let timeline = Rc::downgrade(timeline);
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() {
                return;
            }
            if let Some(timeline) = timeline.upgrade() {
                timeline.edit_motion(|state| apply(state, button.is_active()), false);
            }
        }
    });
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

fn connect_slider(
    slider: &FilledSlider,
    syncing: &Rc<Cell<bool>>,
    timeline: &Rc<Timeline>,
    apply: impl Fn(&mut MotionState, f64) + 'static,
) {
    slider.on_interaction(
        {
            let timeline = Rc::downgrade(timeline);
            Rc::new(move || {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.begin_edit();
                }
            })
        },
        {
            let timeline = Rc::downgrade(timeline);
            Rc::new(move || {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.finish_edit();
                }
            })
        },
    );
    slider.on_change({
        let timeline = Rc::downgrade(timeline);
        let syncing = syncing.clone();
        Rc::new(move |value| {
            if syncing.get() {
                return;
            }
            if let Some(timeline) = timeline.upgrade() {
                timeline.edit_motion(|state| apply(state, value), false);
            }
        })
    });
}

type SliderSpec = (f64, f64, f64, f64, usize);
type ColorChanged = Rc<RefCell<Option<Rc<dyn Fn([f64; 4])>>>>;
type RepaintSlot = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
type PadListener = Rc<RefCell<Option<Rc<dyn Fn(f64, f64)>>>>;
type Interaction = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

fn slider(label: &str, spec: SliderSpec, suffix: &'static str, scale: f64) -> FilledSlider {
    let (min, max, initial, step, decimals) = spec;
    FilledSlider::new(&t(label), min, max, initial, step, decimals, scale).with_suffix(suffix)
}

fn toggle_button(label: &str, _scale: f64) -> ToggleButton {
    let button = ToggleButton::with_label(&t(label));
    button.add_css_class("recording-editor-zoom-easing-btn");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn toggle_row<T>(values: &[(T, ToggleButton)]) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.add_css_class("recording-editor-zoom-easing");
    row.set_hexpand(true);
    row.set_homogeneous(true);
    for (_, button) in values {
        row.append(button);
    }
    row
}

fn group_toggles(buttons: &[ToggleButton]) {
    if let Some(first) = buttons.first() {
        for button in &buttons[1..] {
            button.set_group(Some(first));
        }
    }
}

fn position_readout(x: f64, y: f64) -> String {
    format!("{:.0}% · {:.0}%", x * 100.0, y * 100.0)
}

fn font_families() -> Vec<String> {
    use pango::prelude::FontMapExt;
    let mut families = pangocairo::FontMap::default()
        .list_families()
        .iter()
        .map(|family| family.name().to_string())
        .collect::<Vec<_>>();
    families.sort();
    families.dedup();
    if !families
        .iter()
        .any(|family| family == crate::typography::UI_FONT_FAMILY)
    {
        families.insert(0, crate::typography::UI_FONT_FAMILY.to_string());
    }
    families
}

struct TextColorControl {
    row: GtkBox,
    swatch: DrawingArea,
    value: Rc<Cell<[f64; 4]>>,
    picker: ColorPicker,
    changed: ColorChanged,
    on_start: Interaction,
    on_end: Interaction,
}

impl TextColorControl {
    fn new(scale: f64, column: &GtkBox) -> Self {
        let row = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
        row.add_css_class("image-editor-bg-custom-row");
        row.set_hexpand(true);
        let swatch = DrawingArea::new();
        swatch.set_content_width(px(28.0, scale));
        swatch.set_content_height(px(28.0, scale));
        swatch.set_valign(Align::Center);
        swatch.set_can_target(false);
        let value = Rc::new(Cell::new([1.0, 1.0, 1.0, 1.0]));
        swatch.set_draw_func({
            let value = value.clone();
            move |area, context, width, height| {
                let color = value.get();
                crate::capture::editor::render::rounded_rect_path(
                    context,
                    0.5,
                    0.5,
                    f64::from(width) - 1.0,
                    f64::from(height) - 1.0,
                    7.0,
                );
                context.set_source_rgb(color[0], color[1], color[2]);
                context.fill_preserve().ok();
                if sections::widget_is_light(area) {
                    context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
                } else {
                    context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
                }
                context.set_line_width(1.0);
                context.stroke().ok();
            }
        });
        let label = Label::new(Some(&t("Color")));
        label.add_css_class("image-editor-bg-custom-kind");
        label.set_hexpand(true);
        label.set_xalign(0.0);
        let edit = Button::with_label(&t("Edit"));
        edit.add_css_class("image-editor-bg-custom-edit");
        edit.set_has_frame(false);
        edit.set_valign(Align::Center);
        edit.set_tooltip_text(Some(&t("Edit text color")));
        row.append(&swatch);
        row.append(&label);
        row.append(&edit);

        let changed: ColorChanged = Rc::new(RefCell::new(None));
        let repaint: RepaintSlot = Rc::new(RefCell::new(None));
        let picker = build_color_picker(
            {
                let value = value.clone();
                Rc::new(move || {
                    let color = value.get();
                    (
                        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    )
                })
            },
            {
                let value = value.clone();
                Rc::new(move |(red, green, blue)| {
                    let mut color = value.get();
                    color[0] = f64::from(red) / 255.0;
                    color[1] = f64::from(green) / 255.0;
                    color[2] = f64::from(blue) / 255.0;
                    value.set(color);
                })
            },
            {
                let value = value.clone();
                let changed = changed.clone();
                let swatch = swatch.downgrade();
                let repaint = repaint.clone();
                Rc::new(move || {
                    if let Some(swatch) = swatch.upgrade() {
                        swatch.queue_draw();
                    }
                    if let Some(repaint) = repaint.borrow().as_ref() {
                        repaint();
                    }
                    if let Some(callback) = changed.borrow().as_ref() {
                        callback(value.get());
                    }
                })
            },
            scale,
        );
        *repaint.borrow_mut() = Some(picker.repaint.clone());
        let on_start: Interaction = Rc::new(RefCell::new(None));
        let on_end: Interaction = Rc::new(RefCell::new(None));

        // The same card the Appearance panel opens: header, close, and the
        // shared picker body, sized and styled by the recording editor sheet.
        let popover = Popover::new();
        popover.add_css_class("recording-editor-custom-popover");
        popover.set_has_arrow(false);
        popover.set_position(PositionType::Left);
        popover.set_parent(column);
        popover.connect_show({
            let edit = edit.clone();
            let column = column.downgrade();
            let repaint = picker.repaint.clone();
            let on_start = on_start.clone();
            move |popover| {
                if let Some(column) = column.upgrade() {
                    if let Some(bounds) = edit.compute_bounds(&column) {
                        popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(
                            0,
                            (bounds.y() + bounds.height() / 2.0).round() as i32,
                            1,
                            1,
                        )));
                    }
                }
                repaint();
                if let Some(callback) = on_start.borrow().as_ref() {
                    callback();
                }
            }
        });
        let body = GtkBox::new(Orientation::Vertical, 0);
        body.add_css_class("recording-editor-custom-body");
        body.set_size_request(px(184.0, scale), -1);
        let header = GtkBox::new(Orientation::Horizontal, 8);
        header.add_css_class("recording-editor-custom-header");
        let title = Label::new(Some(&t("Text color")));
        title.add_css_class("recording-editor-custom-title");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let close = Button::new();
        close.add_css_class("recording-editor-custom-close");
        close.set_has_frame(false);
        close.set_tooltip_text(Some(&t("Close")));
        let close_icon = Image::from_icon_name("window-close-symbolic");
        close_icon.set_pixel_size(13);
        close.set_child(Some(&close_icon));
        close.connect_clicked({
            let popover = popover.clone();
            move |_| popover.popdown()
        });
        header.append(&title);
        header.append(&close);
        body.append(&header);
        body.append(&picker.widget);
        popover.set_child(Some(&body));
        popover.connect_closed({
            let on_end = on_end.clone();
            move |_| {
                if let Some(callback) = on_end.borrow().as_ref() {
                    callback();
                }
            }
        });
        row.connect_destroy({
            let popover = popover.downgrade();
            move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.unparent();
                }
            }
        });
        edit.connect_clicked(move |_| popover.popup());

        Self {
            row,
            swatch,
            value,
            picker,
            changed,
            on_start,
            on_end,
        }
    }

    fn set_color(&self, color: [f64; 4]) {
        if self.value.replace(color) != color {
            self.swatch.queue_draw();
            (self.picker.repaint)();
        }
    }

    fn connect_changed(&self, callback: impl Fn([f64; 4]) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(callback));
    }

    fn on_interaction(&self, start: Rc<dyn Fn()>, end: Rc<dyn Fn()>) {
        *self.on_start.borrow_mut() = Some(start);
        *self.on_end.borrow_mut() = Some(end);
    }
}

#[derive(Clone)]
struct TextPad {
    area: DrawingArea,
    position: Rc<Cell<(f64, f64)>>,
    canvas: Rc<Cell<bool>>,
    listener: PadListener,
    on_start: Interaction,
    on_end: Interaction,
}

impl TextPad {
    fn new(scale: f64) -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, px(152.0, scale));
        area.set_hexpand(true);
        area.add_css_class("image-editor-position-pad");
        area.set_cursor_from_name(Some("crosshair"));
        let position = Rc::new(Cell::new((
            DEFAULT_MOTION_TEXT_POS_X,
            DEFAULT_MOTION_TEXT_POS_Y,
        )));
        let canvas = Rc::new(Cell::new(true));
        let listener: PadListener = Rc::new(RefCell::new(None));
        let on_start: Interaction = Rc::new(RefCell::new(None));
        let on_end: Interaction = Rc::new(RefCell::new(None));
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
            let position = position.clone();
            move |area, context, width, height| {
                let (x, y) = position.get();
                sections::paint_position_pad(
                    area,
                    context,
                    width,
                    height,
                    &(x * 2.0 - 1.0, y * 2.0 - 1.0),
                );
            }
        });
        let apply: Rc<dyn Fn(f64, f64, bool)> = {
            let area = area.downgrade();
            let position = position.clone();
            let canvas = canvas.clone();
            let listener = listener.clone();
            Rc::new(move |x, y, reset| {
                let Some(area) = area.upgrade() else {
                    return;
                };
                let width = f64::from(area.width().max(1));
                let height = f64::from(area.height().max(1));
                let (min, max) = if canvas.get() {
                    (MIN_MOTION_TEXT_CANVAS_POS, 1.0)
                } else {
                    (MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS)
                };
                let margin = 19.0_f64.min(width * 0.5).min(height * 0.5);
                let inner_w = (width - margin * 2.0).max(1.0);
                let inner_h = (height - margin * 2.0).max(1.0);
                let value = if reset {
                    (DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y)
                } else {
                    (
                        ((x - margin) / inner_w).clamp(min, max),
                        ((y - margin) / inner_h).clamp(min, max),
                    )
                };
                position.set(value);
                area.queue_draw();
                if let Some(callback) = listener.borrow().as_ref() {
                    callback(value.0, value.1);
                }
            })
        };
        let click = GestureClick::new();
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
        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let apply = apply.clone();
            let start = start.clone();
            move |gesture, _, _| {
                start();
                if let Some((x, y)) = gesture.start_point() {
                    apply(x, y, false);
                }
            }
        });
        drag.connect_drag_update({
            let apply = apply.clone();
            move |gesture, dx, dy| {
                if let Some((x, y)) = gesture.start_point() {
                    apply(x + dx, y + dy, false);
                }
            }
        });
        drag.connect_drag_end({
            let end = end.clone();
            move |_, _, _| end()
        });
        area.add_controller(drag);
        let pad = Self {
            area,
            position,
            canvas,
            listener,
            on_start,
            on_end,
        };
        pad.set_attachment(true);
        pad
    }

    fn set_attachment(&self, canvas: bool) {
        self.canvas.set(canvas);
        self.area.set_tooltip_text(Some(&t(if canvas {
            "Drag to place the headline on the composition; double-click to reset"
        } else {
            "Drag to place the title on the image; double-click to reset"
        })));
        self.area.queue_draw();
    }

    fn set_position(&self, x: f64, y: f64) {
        let (min, max) = if self.canvas.get() {
            (MIN_MOTION_TEXT_CANVAS_POS, 1.0)
        } else {
            (MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS)
        };
        self.position.set((x.clamp(min, max), y.clamp(min, max)));
        self.area.queue_draw();
    }

    fn connect_value_changed(&self, listener: impl Fn(f64, f64) + 'static) {
        *self.listener.borrow_mut() = Some(Rc::new(listener));
    }

    fn on_interaction(&self, start: Rc<dyn Fn()>, end: Rc<dyn Fn()>) {
        *self.on_start.borrow_mut() = Some(start);
        *self.on_end.borrow_mut() = Some(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_page_edits_the_selected_title() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let timeline = Rc::new(super::super::timeline::build(1.0, true));
            let column = GtkBox::new(Orientation::Vertical, 0);
            let controls = Rc::new(TextControls::build(1.0, &column));
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
            assert!(controls.empty.is_visible());
            assert!(!controls.editor.is_visible());

            controls.add.emit_clicked();
            let state = timeline.motion_state();
            assert_eq!(state.text_segments.len(), 1);
            assert!(controls.editor.is_visible());
            assert!(!controls.empty.is_visible());

            controls.text_view.buffer().set_text("Hello");
            assert_eq!(
                timeline
                    .motion_state()
                    .selected_text_segment()
                    .unwrap()
                    .text,
                "Hello"
            );

            controls.bold.set_active(false);
            assert!(
                !timeline
                    .motion_state()
                    .selected_text_segment()
                    .unwrap()
                    .format
                    .bold
            );
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
