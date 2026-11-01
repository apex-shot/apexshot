//! Visual selector for obfuscation methods.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Popover};
use std::cell::Cell;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::ui_support::px;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{AnnotationAction, ObfuscateMethod};
use crate::i18n::t;

const METHODS: [(ObfuscateMethod, &str); 3] = [
    (ObfuscateMethod::Pixelate, "Pixelate"),
    (ObfuscateMethod::Blur, "Blur"),
    (ObfuscateMethod::Blackout, "Blackout"),
];

pub(super) fn active_method(st: &EditorState) -> ObfuscateMethod {
    match st.selected_action() {
        Some(AnnotationAction::Obfuscate { method, .. }) => *method,
        _ => st.obfuscate_method(),
    }
}

fn icon(scale: f64, method: Rc<Cell<ObfuscateMethod>>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(px(28.0, scale));
    area.set_content_height(px(22.0, scale));
    area.set_can_target(false);
    area.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let set_alpha = |alpha: f64| {
            cr.set_source_rgba(
                color.red().into(),
                color.green().into(),
                color.blue().into(),
                f64::from(color.alpha()) * alpha,
            );
        };
        match method.get() {
            ObfuscateMethod::Pixelate => {
                let side = 4.0 * scale;
                for row in 0..3 {
                    for col in 0..3 {
                        set_alpha([0.95, 0.55, 0.75][(row + col) % 3]);
                        cr.rectangle(
                            cx + (col as f64 - 1.5) * side,
                            cy + (row as f64 - 1.5) * side,
                            side - scale,
                            side - scale,
                        );
                        let _ = cr.fill();
                    }
                }
            }
            ObfuscateMethod::Blur => {
                for (radius, alpha) in [(9.0, 0.08), (7.0, 0.13), (5.0, 0.22), (3.0, 0.55)] {
                    set_alpha(alpha);
                    cr.arc(cx, cy, radius * scale, 0.0, std::f64::consts::TAU);
                    let _ = cr.fill();
                }
            }
            ObfuscateMethod::Blackout => {
                set_alpha(1.0);
                cr.rectangle(
                    cx - 7.0 * scale,
                    cy - 7.0 * scale,
                    14.0 * scale,
                    14.0 * scale,
                );
                let _ = cr.fill();
            }
        }
    });
    area
}

pub(super) struct EffectMode {
    pub button: Button,
    popover: Popover,
    method: Rc<Cell<ObfuscateMethod>>,
    icon: DrawingArea,
    label: Label,
    options: Vec<Button>,
}

impl EffectMode {
    pub fn new(scale: f64, host: &Rc<EditorHost>) -> Self {
        let method = Rc::new(Cell::new(ObfuscateMethod::Pixelate));
        let button = Button::new();
        button.add_css_class("image-editor-stroke-trigger");
        button.add_css_class("image-editor-effect-mode-trigger");
        let content = GtkBox::new(Orientation::Horizontal, px(5.0, scale));
        let preview = icon(scale, method.clone());
        content.append(&preview);
        let label = Label::new(Some(&t("Pixelate")));
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
        for (value, name) in METHODS {
            let option = Button::new();
            option.add_css_class("image-editor-stroke-option");
            option.add_css_class("image-editor-effect-mode-option");
            option.set_valign(Align::Center);
            let description = t(name);
            option.set_tooltip_text(Some(&description));
            option.update_property(&[gtk4::accessible::Property::Label(&description)]);
            option.set_child(Some(&icon(scale, Rc::new(Cell::new(value)))));
            option.connect_clicked({
                let host = host.clone();
                let popover = popover.downgrade();
                move |_| {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    host.mutate_effects(|st| {
                        st.set_obfuscate_method(value);
                        true
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
            method,
            icon: preview,
            label,
            options,
        }
    }

    pub fn anchor_to(&self, bar: &GtkBox) {
        self.popover.set_parent(bar);
    }

    pub fn sync(&self, st: &EditorState) {
        let method = active_method(st);
        self.method.set(method);
        self.icon.queue_draw();
        let label = METHODS
            .iter()
            .find(|(value, _)| *value == method)
            .map(|(_, label)| *label)
            .unwrap_or("Pixelate");
        self.label.set_text(&t(label));
        self.button.set_tooltip_text(Some(&t(label)));
        for (option, (value, _)) in self.options.iter().zip(METHODS) {
            if value == method {
                option.add_css_class("selected");
            } else {
                option.remove_css_class("selected");
            }
            option.update_state(&[gtk4::accessible::State::Pressed(if value == method {
                gtk4::AccessibleTristate::True
            } else {
                gtk4::AccessibleTristate::False
            })]);
        }
    }
}

impl Drop for EffectMode {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{Rect, Tool};
    use image::RgbaImage;

    #[test]
    fn icon_menu_tracks_selected_and_default_methods() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let mut st = EditorState::new(RgbaImage::new(32, 32));
            st.selected_tool = Tool::Select;
            st.obfuscate_method = ObfuscateMethod::Pixelate;
            st.push_action(AnnotationAction::Obfuscate {
                rect: Rect {
                    x: 2,
                    y: 2,
                    width: 10,
                    height: 10,
                },
                method: ObfuscateMethod::Blur,
                amount: 8.0,
            });
            *host.editor.borrow_mut() = Some(st);
            let control = EffectMode::new(1.0, &host);
            let bar = GtkBox::new(Orientation::Horizontal, 0);
            bar.append(&control.button);
            control.anchor_to(&bar);
            assert_eq!(control.popover.parent(), Some(bar.upcast()));
            assert_eq!(control.popover.position(), gtk4::PositionType::Bottom);
            host.read(|st| control.sync(st));
            assert!(control.options[1].has_css_class("selected"));
            assert_eq!(control.label.text(), "Blur");
            control.options[2].emit_clicked();
            host.read(|st| {
                assert_eq!(active_method(st), ObfuscateMethod::Blackout);
                assert_eq!(st.obfuscate_method(), ObfuscateMethod::Blackout);
                control.sync(st);
            });
            assert!(control.options[2].has_css_class("selected"));
            assert_eq!(
                control.options[2].tooltip_text().as_deref(),
                Some("Blackout")
            );
            host.undo();
            host.mutate(|st| st.selected_action_index = Some(0));
            host.read(|st| {
                assert_eq!(active_method(st), ObfuscateMethod::Blur);
                control.sync(st);
            });
            assert!(control.options[1].has_css_class("selected"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
