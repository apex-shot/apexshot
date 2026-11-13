//! Icon-based arrow style selector for the annotation bar.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Orientation, Popover};
use std::cell::Cell;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::ui_support::px;
use crate::capture::editor::render::draw_arrow;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{ArrowStyle, DrawColor, Point};
use crate::i18n::t;

const STYLES: [(ArrowStyle, &str); 4] = [
    (ArrowStyle::Standard, "Standard"),
    (ArrowStyle::Fancy, "Fancy"),
    (ArrowStyle::Curved, "Curved"),
    (ArrowStyle::Double, "Double"),
];

fn preview(scale: f64, style: Rc<Cell<ArrowStyle>>) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.set_content_width(px(28.0, scale));
    preview.set_content_height(px(22.0, scale));
    preview.set_can_target(false);
    preview.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let (w, h) = (f64::from(width), f64::from(height));
        let start = Point {
            x: w * 0.16,
            y: h * 0.76,
        };
        let end = Point {
            x: w * 0.84,
            y: h * 0.22,
        };
        let style = style.get();
        let controls = if matches!(style, ArrowStyle::Curved | ArrowStyle::Double) {
            Some(vec![
                start,
                Point {
                    x: w * 0.22,
                    y: h * 0.18,
                },
                end,
            ])
        } else {
            None
        };
        draw_arrow(
            cr,
            start,
            end,
            DrawColor::new(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            ),
            1.6 * scale,
            style,
            controls,
            false,
        );
    });
    preview
}

pub(super) struct ArrowControl {
    pub button: Button,
    popover: Popover,
    style: Rc<Cell<ArrowStyle>>,
    preview: DrawingArea,
    options: Vec<Button>,
}

impl ArrowControl {
    pub fn new(scale: f64, host: &Rc<EditorHost>) -> Self {
        let style = Rc::new(Cell::new(ArrowStyle::Standard));
        let button = Button::new();
        button.add_css_class("image-editor-stroke-trigger");
        button.add_css_class("image-editor-arrow-trigger");
        let content = GtkBox::new(Orientation::Horizontal, px(5.0, scale));
        let icon = preview(scale, style.clone());
        content.append(&icon);
        let label = gtk4::Label::new(Some(&t("Arrow")));
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
        for (value, label) in STYLES {
            let option = Button::new();
            option.add_css_class("image-editor-stroke-option");
            option.add_css_class("image-editor-arrow-option");
            option.set_valign(Align::Center);
            let description = t(label);
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.set_child(Some(&preview(scale, Rc::new(Cell::new(value)))));
            option.connect_clicked({
                let host = host.clone();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    host.mutate(|st| {
                        st.set_arrow_style(value);
                        st.set_selected_arrow_style_with_controls(value);
                    });
                }
            });
            list.append(&option);
            options.push(option);
        }
        popover.set_child(Some(&list));
        button.connect_clicked({
            let popover = popover.clone();
            move |_| popover.popup()
        });
        Self {
            button,
            popover,
            style,
            preview: icon,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, state: &EditorState) {
        let value = state.selected_arrow_style().unwrap_or(state.arrow_style);
        self.style.set(value);
        self.preview.queue_draw();
        let label = STYLES
            .iter()
            .find(|(style, _)| *style == value)
            .map(|(_, label)| *label)
            .unwrap_or("Standard");
        let description = format!("{}: {}", t("Arrow style"), t(label));
        self.button.set_tooltip_text(Some(&description));
        self.button
            .update_property(&[gtk4::accessible::Property::Label(&description)]);
        for (option, (style, _)) in self.options.iter().zip(STYLES) {
            if style == value {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(&[gtk4::accessible::State::Pressed(if style == value {
                gtk4::AccessibleTristate::True
            } else {
                gtk4::AccessibleTristate::False
            })]);
        }
    }
}

impl Drop for ArrowControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::AnnotationAction;
    use image::RgbaImage;

    #[test]
    fn icons_apply_and_sync_arrow_styles() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let mut state = EditorState::new(RgbaImage::new(32, 32));
            state.push_action(AnnotationAction::Arrow {
                start: Point { x: 2.0, y: 2.0 },
                end: Point { x: 25.0, y: 25.0 },
                color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
                stroke_size: 4.0,
                style: ArrowStyle::Standard,
                control_points: None,
                shadow: false,
                reversed: false,
            });
            state.selected_action_index = Some(0);
            *host.editor.borrow_mut() = Some(state);
            let control = ArrowControl::new(1.0, &host);
            let bar = GtkBox::new(Orientation::Horizontal, 0);
            bar.append(&control.button);
            control.anchor_to(&bar);
            assert_eq!(control.popover.parent(), Some(bar.upcast()));
            assert_eq!(control.popover.position(), gtk4::PositionType::Bottom);
            host.read(|st| control.sync(st));
            assert!(control.options[0].has_css_class("selected"));
            control.options[2].emit_clicked();
            host.read(|st| {
                assert_eq!(st.selected_arrow_style(), Some(ArrowStyle::Curved));
                assert_eq!(st.arrow_style, ArrowStyle::Curved);
                control.sync(st);
            });
            assert!(control.options[2].has_css_class("selected"));
            assert!(!control.options[0].has_css_class("selected"));
            host.undo();
            host.read(|st| {
                assert!(matches!(
                    st.actions[0],
                    AnnotationAction::Arrow {
                        style: ArrowStyle::Standard,
                        ..
                    }
                ));
            });
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn reversed_arrow_state_survives_selection_and_history() {
        let mut state = EditorState::new(RgbaImage::new(100, 100));
        state.selected_tool = crate::capture::editor::types::Tool::Arrow;
        state.inverse_arrow_direction = true;
        state.begin_drag(Point { x: 10.0, y: 10.0 });
        state.update_drag(Point { x: 80.0, y: 80.0 });
        let arrow = state.finalize_drag_action().unwrap();
        assert!(matches!(
            arrow,
            AnnotationAction::Arrow { reversed: true, .. }
        ));
        state.push_action(arrow);
        state.selected_action_index = Some(0);
        assert_eq!(state.selected_arrow_reversed(), Some(true));
        assert!(state.reverse_selected_arrow_action());
        assert_eq!(state.selected_arrow_reversed(), Some(false));
        state.selected_action_index = None;
        state.selected_action_index = Some(0);
        assert_eq!(state.selected_arrow_reversed(), Some(false));
        assert!(state.undo());
        state.selected_action_index = Some(0);
        assert_eq!(state.selected_arrow_reversed(), Some(true));
        assert!(state.redo());
        state.selected_action_index = Some(0);
        assert_eq!(state.selected_arrow_reversed(), Some(false));
    }
}
