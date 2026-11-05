//! Compact stroke presets for the annotation option bars.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Popover};
use std::cell::Cell;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::ui_support::px;
use crate::capture::editor::pen_weight::{HighlighterMode, PenWeight};
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{AnnotationAction, Tool};
use crate::i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrokeKind {
    Pen,
    Shape,
    Highlighter,
}

impl StrokeKind {
    fn width(self, weight: PenWeight) -> f64 {
        match self {
            Self::Pen => weight.pen_stroke_width(),
            Self::Highlighter => weight.highlighter_stroke_width(),
            Self::Shape => [2.0, 4.0, 7.0, 12.0][weight.index()],
        }
    }
}

fn active_kind(st: &EditorState) -> StrokeKind {
    match st.selected_action() {
        Some(AnnotationAction::Pen { .. }) => StrokeKind::Pen,
        Some(AnnotationAction::Highlighter { .. }) => StrokeKind::Highlighter,
        Some(
            AnnotationAction::Line { .. }
            | AnnotationAction::Arrow { .. }
            | AnnotationAction::Box { .. }
            | AnnotationAction::Circle { .. },
        ) => StrokeKind::Shape,
        _ => match st.selected_tool {
            Tool::Pen => StrokeKind::Pen,
            Tool::Highlighter => StrokeKind::Highlighter,
            _ => StrokeKind::Shape,
        },
    }
}

fn active_width(st: &EditorState, kind: StrokeKind) -> Option<f64> {
    if let Some(width) = st.selected_action_stroke_size() {
        return Some(width);
    }
    if kind == StrokeKind::Highlighter && st.highlighter_mode == HighlighterMode::TextAware {
        None
    } else {
        Some(st.stroke_size)
    }
}

fn apply_weight(st: &mut EditorState, kind: StrokeKind, weight: PenWeight) {
    if kind == StrokeKind::Highlighter {
        st.set_highlighter_weight(weight);
        st.set_stroke_size(kind.width(weight));
    } else {
        if kind == StrokeKind::Pen {
            st.set_pen_weight_and_apply(weight);
        }
        st.set_active_size_without_rebuild(kind.width(weight));
    }
}

fn stroke_preview(
    scale: f64,
    kind: Rc<Cell<StrokeKind>>,
    weight: Rc<Cell<Option<usize>>>,
) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.add_css_class("image-editor-stroke-preview");
    preview.set_content_width(px(28.0, scale));
    preview.set_content_height(px(22.0, scale));
    preview.set_can_target(false);
    preview.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        let widths = [1.5, 2.7, 4.1, 6.0];
        cr.set_line_width(widths[weight.get().unwrap_or(1).min(3)] * scale);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        cr.set_line_join(gtk4::cairo::LineJoin::Round);
        let w = f64::from(width);
        let h = f64::from(height);
        if kind.get() == StrokeKind::Shape {
            cr.move_to(w * 0.19, h * 0.5);
            cr.line_to(w * 0.81, h * 0.5);
        } else {
            cr.move_to(w * 0.15, h * 0.64);
            cr.curve_to(w * 0.26, h * 0.22, w * 0.44, h * 0.20, w * 0.42, h * 0.56);
            cr.curve_to(w * 0.40, h * 0.80, w * 0.62, h * 0.20, w * 0.64, h * 0.55);
            cr.curve_to(w * 0.65, h * 0.75, w * 0.78, h * 0.65, w * 0.86, h * 0.48);
        }
        let _ = cr.stroke();
    });
    preview
}

pub(super) struct StrokeControl {
    pub button: Button,
    popover: Popover,
    sync: Rc<dyn Fn(&EditorState)>,
    #[cfg(test)]
    options: Vec<Button>,
}

impl StrokeControl {
    pub fn new(scale: f64, host: &Rc<EditorHost>) -> Self {
        let kind = Rc::new(Cell::new(StrokeKind::Pen));
        let current = Rc::new(Cell::new(None));
        let button = Button::new();
        button.add_css_class("image-editor-stroke-trigger");
        let content = GtkBox::new(Orientation::Horizontal, px(5.0, scale));
        let preview = stroke_preview(scale, kind.clone(), current.clone());
        content.append(&preview);
        let label = Label::new(Some(&t("Stroke")));
        label.add_css_class("image-editor-subbar-pill-label");
        content.append(&label);
        let chevron = gtk4::Image::from_icon_name("chevron-down-regular");
        chevron.set_pixel_size(px(10.0, scale));
        chevron.add_css_class("image-editor-subbar-pill-arrow");
        content.append(&chevron);
        button.set_child(Some(&content));

        let popover = Popover::new();
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_position(gtk4::PositionType::Bottom);
        popover.add_css_class("image-editor-subbar-popover");
        popover.add_css_class("image-editor-stroke-popover");
        let list = GtkBox::new(Orientation::Horizontal, px(3.0, scale));
        list.add_css_class("image-editor-stroke-menu");
        let mut options = Vec::new();
        let mut previews = Vec::new();
        for weight in PenWeight::ALL {
            let option = Button::new();
            option.add_css_class("image-editor-stroke-option");
            option.set_valign(Align::Center);
            let icon = stroke_preview(
                scale,
                kind.clone(),
                Rc::new(Cell::new(Some(weight.index()))),
            );
            option.set_child(Some(&icon));
            option.connect_clicked({
                let host = host.clone();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    host.mutate(|st| {
                        let kind = active_kind(st);
                        apply_weight(st, kind, weight);
                    });
                }
            });
            list.append(&option);
            options.push(option);
            previews.push(icon);
        }
        popover.set_child(Some(&list));
        button.connect_clicked({
            let popover = popover.clone();
            move |_| popover.popup()
        });

        let sync = {
            let options = options.clone();
            let button = button.downgrade();
            Rc::new(move |st: &EditorState| {
                let active_kind = active_kind(st);
                let width = active_width(st, active_kind);
                let selected = width.and_then(|width| {
                    PenWeight::ALL
                        .iter()
                        .position(|weight| (active_kind.width(*weight) - width).abs() < 0.001)
                });
                let preview_index = width.and_then(|width| {
                    PenWeight::ALL
                        .iter()
                        .min_by(|a, b| {
                            (active_kind.width(**a) - width)
                                .abs()
                                .total_cmp(&(active_kind.width(**b) - width).abs())
                        })
                        .map(|weight| weight.index())
                });
                kind.set(active_kind);
                current.set(preview_index);
                preview.queue_draw();
                if let Some(button) = button.upgrade() {
                    let description = match width {
                        Some(width) => format!("{}: {} px", t("Stroke"), width),
                        None => format!("{}: {}", t("Stroke"), t("Automatic")),
                    };
                    button.set_tooltip_text(Some(&description));
                    button.update_property(&[gtk4::accessible::Property::Label(&description)]);
                }
                for (index, option) in options.iter().enumerate() {
                    if selected == Some(index) {
                        option.add_css_class("selected");
                    } else {
                        option.remove_css_class("selected");
                    }
                    option.update_state(&[gtk4::accessible::State::Pressed(
                        if selected == Some(index) {
                            gtk4::AccessibleTristate::True
                        } else {
                            gtk4::AccessibleTristate::False
                        },
                    )]);
                    let weight = PenWeight::ALL[index];
                    let description =
                        format!("{} ({} px)", t(weight.label()), active_kind.width(weight));
                    option.set_tooltip_text(Some(&description));
                    option.update_property(&[gtk4::accessible::Property::Label(&description)]);
                    previews[index].queue_draw();
                }
            })
        };
        Self {
            button,
            popover,
            sync,
            #[cfg(test)]
            options,
        }
    }

    pub fn sync(&self, state: &EditorState) {
        (self.sync)(state);
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }
}

impl Drop for StrokeControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{ArrowStyle, DrawColor, Point, Rect};
    use image::RgbaImage;

    fn state(tool: Tool) -> EditorState {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.selected_tool = tool;
        state
    }

    fn host(tool: Tool) -> Rc<EditorHost> {
        let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
        *host.editor.borrow_mut() = Some(state(tool));
        host
    }

    fn actions() -> Vec<AnnotationAction> {
        let color = DrawColor::new(1.0, 0.0, 0.0, 1.0);
        let start = Point { x: 2.0, y: 3.0 };
        let end = Point { x: 20.0, y: 20.0 };
        vec![
            AnnotationAction::Pen {
                points: vec![start, end],
                color,
                stroke_size: 5.0,
            },
            AnnotationAction::Highlighter {
                points: vec![start, end],
                color,
                stroke_size: 16.0,
            },
            AnnotationAction::Line {
                start,
                end,
                color,
                stroke_size: 4.0,
                shadow: false,
            },
            AnnotationAction::Arrow {
                start,
                end,
                color,
                stroke_size: 4.0,
                style: ArrowStyle::Standard,
                control_points: None,
                shadow: false,
                reversed: false,
            },
            AnnotationAction::Box {
                rect: Rect {
                    x: 2,
                    y: 3,
                    width: 18,
                    height: 17,
                },
                color,
                stroke_size: 4.0,
                shadow: false,
            },
            AnnotationAction::Circle {
                rect: Rect {
                    x: 2,
                    y: 3,
                    width: 18,
                    height: 17,
                },
                color,
                stroke_size: 4.0,
                shadow: false,
            },
        ]
    }

    #[test]
    fn preset_widths_follow_the_existing_tool_scales() {
        let widths = |kind: StrokeKind| PenWeight::ALL.map(|weight| kind.width(weight));
        assert_eq!(widths(StrokeKind::Pen), [3.0, 5.0, 8.0, 12.0]);
        assert_eq!(widths(StrokeKind::Shape), [2.0, 4.0, 7.0, 12.0]);
        assert_eq!(widths(StrokeKind::Highlighter), [10.0, 16.0, 24.0, 32.0]);
    }

    #[test]
    fn presets_update_each_tool_and_selected_action_with_undo() {
        for action in actions() {
            let mut state = state(Tool::Select);
            state.push_action(action);
            state.selected_action_index = Some(0);
            let before = state.selected_action_stroke_size().unwrap();
            let kind = active_kind(&state);
            apply_weight(&mut state, kind, PenWeight::Large);
            let expected = kind.width(PenWeight::Large);
            assert_eq!(state.stroke_size, expected);
            assert_eq!(state.selected_action_stroke_size(), Some(expected));
            assert!(state.undo());
            state.selected_action_index = Some(0);
            assert_eq!(state.selected_action_stroke_size(), Some(before));
            assert!(state.redo());
            state.selected_action_index = Some(0);
            assert_eq!(state.selected_action_stroke_size(), Some(expected));
        }
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Arrow,
            Tool::Box,
            Tool::Circle,
            Tool::Highlighter,
        ] {
            let mut state = state(tool);
            let kind = active_kind(&state);
            apply_weight(&mut state, kind, PenWeight::Small);
            assert_eq!(state.stroke_size, kind.width(PenWeight::Small));
        }
    }

    #[test]
    fn auto_highlighter_has_no_fixed_width_until_a_preset_is_picked() {
        let mut state = state(Tool::Highlighter);
        state.highlighter_mode = HighlighterMode::TextAware;
        assert_eq!(active_width(&state, StrokeKind::Highlighter), None);
        apply_weight(&mut state, StrokeKind::Highlighter, PenWeight::Medium);
        assert_eq!(state.highlighter_mode, HighlighterMode::Freehand);
        assert_eq!(active_width(&state, StrokeKind::Highlighter), Some(16.0));
    }

    #[test]
    fn preset_buttons_sync_without_changing_legacy_values() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = host(Tool::Pen);
            let control = StrokeControl::new(1.0, &host);
            host.mutate(|st| st.stroke_size = 6.0);
            host.read(|st| control.sync(st));
            assert!(control
                .options
                .iter()
                .all(|option| !option.has_css_class("selected")));
            assert_eq!(host.read(|st| st.stroke_size), Some(6.0));
            control.options[2].emit_clicked();
            host.read(|st| control.sync(st));
            assert_eq!(host.read(|st| st.stroke_size), Some(8.0));
            assert!(control.options[2].has_css_class("selected"));
            assert_eq!(
                control.options[2].tooltip_text().as_deref(),
                Some("Thick (8 px)")
            );
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn menu_anchors_to_bar_and_matches_its_height() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let display = gtk4::gdk::Display::default().unwrap();
            let css = include_str!("image_editor.css");
            let start = css.find(".image-editor-sub-bar {").unwrap();
            let end = css.find(".image-editor-subbar-menu {").unwrap();
            for scale in [1.0, 1.25] {
                let provider = gtk4::CssProvider::new();
                provider
                    .load_from_data(&crate::css_scale::scale_px_lengths(&css[start..end], scale));
                gtk4::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
                );
                let host = host(Tool::Pen);
                let control = StrokeControl::new(scale, &host);
                let bar = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
                bar.add_css_class("image-editor-sub-bar");
                let color = Button::new();
                color.add_css_class("image-editor-color-trigger");
                color.set_child(Some(
                    &DrawingArea::builder()
                        .content_width(px(20.0, scale))
                        .content_height(px(20.0, scale))
                        .build(),
                ));
                bar.append(&color);
                bar.append(&control.button);
                let window = gtk4::Window::new();
                window.set_child(Some(&bar));
                gtk4::prelude::WidgetExt::realize(&window);
                let before = bar.measure(Orientation::Vertical, -1).1;
                let width_before = bar.measure(Orientation::Horizontal, -1).1;
                control.anchor_to(&bar);
                assert_eq!(control.popover.parent(), Some(bar.clone().upcast()));
                assert_eq!(control.popover.position(), gtk4::PositionType::Bottom);
                control.popover.set_visible(true);
                let after = bar.measure(Orientation::Vertical, -1).1;
                let width_after = bar.measure(Orientation::Horizontal, -1).1;
                let menu_height = control.popover.measure(Orientation::Vertical, -1).1;
                assert_eq!(after, before, "popover must not change the bar height");
                assert_eq!(
                    width_after, width_before,
                    "popover must not change the bar width"
                );
                assert!(
                    (menu_height - px(6.0, scale) - before).abs() <= 1,
                    "scale {scale}"
                );
                control.popover.set_visible(false);
                window.set_child(None::<&gtk4::Widget>);
                window.destroy();
                gtk4::style_context_remove_provider_for_display(&display, &provider);
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn stroke_menu_styles_parse() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let css = include_str!("image_editor.css");
            let start = css.find("button.image-editor-stroke-trigger").unwrap();
            let end = css.find("popover.image-editor-subbar-popover").unwrap();
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(std::cell::RefCell::new(Vec::new()));
            let captured = errors.clone();
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(&css[start..end]);
            let result = errors.borrow().clone();
            result
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
        assert!(errors.is_empty(), "stroke CSS errors: {errors:?}");
    }
}
