//! Compact visual menus for number marker style and size.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Popover};
use std::cell::Cell;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::ui_support::px;
use crate::capture::editor::numbering_style::{NumberSize, NumberingStyle};
use crate::capture::editor::render::draw_number;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{DrawColor, Point};
use crate::i18n::t;

#[derive(Clone, Copy)]
pub(super) enum NumberControlKind {
    Style,
    Size,
}

fn style_sample(style: NumberingStyle) -> String {
    [1, 2, 3]
        .into_iter()
        .map(|number| style.format(number))
        .collect::<Vec<_>>()
        .join(" ")
}

fn size_preview(scale: f64, size: Rc<Cell<NumberSize>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(px(28.0, scale));
    area.set_content_height(px(22.0, scale));
    area.set_can_target(false);
    area.set_draw_func(move |widget, cr, width, height| {
        let size = size.get();
        let color = widget.color();
        let radius = [5.4, 6.9, 8.4, 9.9][size.index()] * scale;
        let factor = radius / size.radius();
        let _ = cr.save();
        cr.translate(f64::from(width) / 2.0, f64::from(height) / 2.0);
        cr.scale(factor, factor);
        draw_number(
            cr,
            Point { x: 0.0, y: 0.0 },
            1,
            DrawColor::new(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                color.alpha().into(),
            ),
            NumberingStyle::Numeric,
            size,
        );
        let _ = cr.restore();
    });
    area
}

pub(super) struct NumberControl {
    pub button: Button,
    popover: Popover,
    kind: NumberControlKind,
    style_sample: Option<Label>,
    current_size: Rc<Cell<NumberSize>>,
    size_preview: Option<DrawingArea>,
    options: Vec<Button>,
}

impl NumberControl {
    pub fn new(scale: f64, host: &Rc<EditorHost>, kind: NumberControlKind) -> Self {
        let button = Button::new();
        button.add_css_class("image-editor-stroke-trigger");
        let content = GtkBox::new(Orientation::Horizontal, px(5.0, scale));
        let current_size = Rc::new(Cell::new(NumberSize::Medium));
        let (trigger_style_label, trigger_size_preview) = match kind {
            NumberControlKind::Style => {
                button.add_css_class("image-editor-number-style-trigger");
                let label = Label::new(Some(&style_sample(NumberingStyle::Numeric)));
                label.add_css_class("image-editor-number-style-sample");
                content.append(&label);
                (Some(label), None)
            }
            NumberControlKind::Size => {
                button.add_css_class("image-editor-number-size-trigger");
                let icon = size_preview(scale, current_size.clone());
                content.append(&icon);
                let label = Label::new(Some(&t("Size")));
                label.add_css_class("image-editor-subbar-pill-label");
                content.append(&label);
                (None, Some(icon))
            }
        };
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
        let count = match kind {
            NumberControlKind::Style => NumberingStyle::ALL.len(),
            NumberControlKind::Size => NumberSize::ALL.len(),
        };
        for index in 0..count {
            let option = Button::new();
            option.add_css_class("image-editor-stroke-option");
            option.set_valign(Align::Center);
            let description = match kind {
                NumberControlKind::Style => {
                    option.add_css_class("image-editor-number-style-option");
                    let style = NumberingStyle::ALL[index];
                    let sample = Label::new(Some(&style_sample(style)));
                    sample.add_css_class("image-editor-number-style-sample");
                    option.set_child(Some(&sample));
                    t(style.label())
                }
                NumberControlKind::Size => {
                    option.add_css_class("image-editor-number-size-option");
                    let size = NumberSize::ALL[index];
                    option.set_child(Some(&size_preview(scale, Rc::new(Cell::new(size)))));
                    t(size.label())
                }
            };
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.connect_clicked({
                let host = host.clone();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    host.mutate(|st| match kind {
                        NumberControlKind::Style => {
                            st.set_numbering_style(NumberingStyle::ALL[index]);
                        }
                        NumberControlKind::Size => {
                            st.set_number_size(NumberSize::ALL[index]);
                        }
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
            kind,
            style_sample: trigger_style_label,
            current_size,
            size_preview: trigger_size_preview,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, state: &EditorState) {
        let (index, description) = match self.kind {
            NumberControlKind::Style => {
                let style = state.active_numbering_style();
                if let Some(label) = &self.style_sample {
                    label.set_text(&style_sample(style));
                }
                (
                    style.index(),
                    format!("{}: {}", t("Numbering style"), t(style.label())),
                )
            }
            NumberControlKind::Size => {
                let size = state.active_number_size();
                self.current_size.set(size);
                if let Some(preview) = &self.size_preview {
                    preview.queue_draw();
                }
                (size.index(), format!("{}: {}", t("Size"), t(size.label())))
            }
        };
        self.button.set_tooltip_text(Some(&description));
        self.button
            .update_property(&[gtk4::accessible::Property::Label(&description)]);
        for (option_index, option) in self.options.iter().enumerate() {
            if option_index == index {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(
                &[gtk4::accessible::State::Pressed(if option_index == index {
                    gtk4::AccessibleTristate::True
                } else {
                    gtk4::AccessibleTristate::False
                })],
            );
        }
    }
}

impl Drop for NumberControl {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{AnnotationAction, Tool};
    use image::RgbaImage;

    fn host() -> Rc<EditorHost> {
        let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        state.selected_tool = Tool::Number;
        *host.editor.borrow_mut() = Some(state);
        host
    }

    #[test]
    fn style_samples_represent_the_actual_sequences() {
        let samples = NumberingStyle::ALL.map(style_sample);
        assert_eq!(samples, ["1 2 3", "A B C", "a b c", "i ii iii"]);
    }

    #[test]
    fn presets_update_new_and_selected_markers_and_undo() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = host();
            let style = NumberControl::new(1.0, &host, NumberControlKind::Style);
            let size = NumberControl::new(1.0, &host, NumberControlKind::Size);
            let bar = GtkBox::new(Orientation::Horizontal, 0);
            bar.append(&style.button);
            bar.append(&size.button);
            style.anchor_to(&bar);
            size.anchor_to(&bar);
            assert_eq!(style.popover.parent(), Some(bar.clone().upcast()));
            assert_eq!(size.popover.parent(), Some(bar.clone().upcast()));
            assert_eq!(style.popover.position(), gtk4::PositionType::Bottom);
            assert_eq!(size.popover.position(), gtk4::PositionType::Bottom);

            style.options[1].emit_clicked();
            size.options[2].emit_clicked();
            host.read(|st| {
                assert_eq!(st.numbering_style, NumberingStyle::Uppercase);
                assert_eq!(st.number_size, NumberSize::Large);
                style.sync(st);
                size.sync(st);
            });
            assert!(style.options[1].has_css_class("selected"));
            assert!(size.options[2].has_css_class("selected"));
            assert_eq!(style.style_sample.as_ref().unwrap().text(), "A B C");
            assert_eq!(size.current_size.get(), NumberSize::Large);

            host.mutate(|st| {
                st.add_number_marker(Point { x: 20.0, y: 20.0 });
                assert!(matches!(
                    st.actions[0],
                    AnnotationAction::Number {
                        style: NumberingStyle::Uppercase,
                        size: NumberSize::Large,
                        ..
                    }
                ));
                st.selected_action_index = Some(0);
            });
            style.options[3].emit_clicked();
            size.options[0].emit_clicked();
            host.read(|st| {
                assert_eq!(st.active_numbering_style(), NumberingStyle::Roman);
                assert_eq!(st.active_number_size(), NumberSize::Small);
                style.sync(st);
                size.sync(st);
            });
            assert!(style.options[3].has_css_class("selected"));
            assert!(size.options[0].has_css_class("selected"));
            assert_eq!(
                style.options[3].tooltip_text().as_deref(),
                Some("i, ii, iii, iv...")
            );
            assert_eq!(size.options[0].tooltip_text().as_deref(), Some("Small"));
            host.mutate(|st| {
                assert!(st.undo());
                st.selected_action_index = Some(0);
            });
            host.read(|st| {
                assert_eq!(st.active_numbering_style(), NumberingStyle::Uppercase);
                assert_eq!(st.active_number_size(), NumberSize::Large);
                style.sync(st);
                size.sync(st);
            });
            assert!(style.options[1].has_css_class("selected"));
            assert!(size.options[2].has_css_class("selected"));
            host.mutate(|st| {
                assert!(st.redo());
                st.selected_action_index = Some(0);
            });
            host.read(|st| {
                assert_eq!(st.active_numbering_style(), NumberingStyle::Roman);
                assert_eq!(st.active_number_size(), NumberSize::Small);
                style.sync(st);
                size.sync(st);
            });
            assert!(style.options[3].has_css_class("selected"));
            assert!(size.options[0].has_css_class("selected"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn number_menus_match_the_bar_height_at_supported_scales() {
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
                let host = host();
                let style = NumberControl::new(scale, &host, NumberControlKind::Style);
                let size = NumberControl::new(scale, &host, NumberControlKind::Size);
                let bar = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
                bar.add_css_class("image-editor-sub-bar");
                bar.append(&style.button);
                bar.append(&size.button);
                style.anchor_to(&bar);
                size.anchor_to(&bar);
                let window = gtk4::Window::new();
                window.set_child(Some(&bar));
                gtk4::prelude::WidgetExt::realize(&window);
                let bar_height = bar.measure(Orientation::Vertical, -1).1;
                let bar_width = bar.measure(Orientation::Horizontal, -1).1;
                for menu in [&style.popover, &size.popover] {
                    menu.set_visible(true);
                    let menu_height = menu.measure(Orientation::Vertical, -1).1;
                    assert!(
                        (menu_height - px(6.0, scale) - bar_height).abs() <= 1,
                        "menu height should match the bar at scale {scale}"
                    );
                    assert_eq!(bar.measure(Orientation::Horizontal, -1).1, bar_width);
                    menu.set_visible(false);
                }
                window.set_child(None::<&gtk4::Widget>);
                window.destroy();
                gtk4::style_context_remove_provider_for_display(&display, &provider);
            }
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
