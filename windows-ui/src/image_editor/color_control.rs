//! Expandable annotation palette shared by the tool option bars.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Grid, Orientation, Revealer};
use std::cell::Cell;
use std::rc::{Rc, Weak};

use super::annotate::EditorHost;
use super::canvas::Canvas;
use super::custom_color::CustomColor;
use super::picker_placement;
use super::ui_support::px;
use crate::capture::editor::color::draw_color_to_rgba_u8;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::DrawColor;
use crate::i18n::t;

const PALETTE: [(&str, u32); 21] = [
    ("Black", 0x121212),
    ("Gray", 0x777777),
    ("Red", 0xf43d2b),
    ("Orange", 0xff963e),
    ("Yellow", 0xffc83d),
    ("Green", 0x5dce75),
    ("Teal", 0x57d0c4),
    ("Blue", 0x39a7ef),
    ("Purple", 0x8950f7),
    ("Pink", 0xf13dae),
    ("White", 0xffffff),
    ("Silver", 0xaaaaaa),
    ("Light gray", 0xd9d9d9),
    ("Light red", 0xffc4c4),
    ("Light orange", 0xffd9b7),
    ("Light yellow", 0xffe9a8),
    ("Light green", 0xc6efcc),
    ("Light teal", 0xbaf1eb),
    ("Light blue", 0xbce2ff),
    ("Light purple", 0xdac7fa),
    ("Light pink", 0xffc2e7),
];

fn palette_color(rgb: u32) -> DrawColor {
    DrawColor::new(
        f64::from((rgb >> 16) & 0xff) / 255.0,
        f64::from((rgb >> 8) & 0xff) / 255.0,
        f64::from(rgb & 0xff) / 255.0,
        1.0,
    )
}

fn active_color(st: &EditorState) -> DrawColor {
    st.get_text_input()
        .map(|input| input.color)
        .or_else(|| st.selected_action_color())
        .unwrap_or(st.selected_color)
}

fn apply_color(host: &EditorHost, color: DrawColor) {
    host.mutate(|st| {
        st.selected_color = color;
        st.set_selected_action_color(color);
    });
}

fn sync_selection(
    color: DrawColor,
    custom_open: bool,
    swatches: &[(DrawColor, Button)],
    custom: &Button,
) {
    let rgba = draw_color_to_rgba_u8(color);
    let preset_match = swatches
        .iter()
        .any(|(value, _)| draw_color_to_rgba_u8(*value) == rgba);
    for (value, button) in swatches {
        if !custom_open && draw_color_to_rgba_u8(*value) == rgba {
            button.add_css_class("selected");
        } else {
            button.remove_css_class("selected");
        }
    }
    if custom_open || !preset_match {
        custom.add_css_class("selected");
    } else {
        custom.remove_css_class("selected");
    }
}

fn color_dot(scale: f64, color: Rc<Cell<DrawColor>>) -> DrawingArea {
    let dot = DrawingArea::new();
    dot.set_content_width(px(20.0, scale));
    dot.set_content_height(px(20.0, scale));
    dot.set_draw_func(move |_, cr, width, height| {
        let size = f64::from(width.min(height));
        let (x, y) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let radius = size / 2.0 - 0.5;
        let _ = cr.save();
        cr.arc(x, y, radius, 0.0, std::f64::consts::TAU);
        cr.clip();
        cr.set_source_rgb(0.88, 0.88, 0.88);
        let _ = cr.paint();
        cr.set_source_rgb(0.65, 0.65, 0.65);
        let half = size / 2.0;
        cr.rectangle(x - half, y - half, half, half);
        cr.rectangle(x, y, half, half);
        let _ = cr.fill();
        let c = color.get();
        cr.set_source_rgba(c.r, c.g, c.b, c.a);
        let _ = cr.paint();
        let _ = cr.restore();
        cr.arc(x, y, radius, 0.0, std::f64::consts::TAU);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.5);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    });
    dot
}

fn rainbow_dot(scale: f64) -> DrawingArea {
    let dot = DrawingArea::new();
    dot.set_content_width(px(20.0, scale));
    dot.set_content_height(px(20.0, scale));
    dot.set_draw_func(|_, cr, width, height| {
        let (x, y) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let radius = f64::from(width.min(height)) / 2.0 - 0.5;
        let colors = [
            (1.0, 0.25, 0.25),
            (1.0, 0.68, 0.18),
            (1.0, 0.9, 0.2),
            (0.26, 0.8, 0.45),
            (0.18, 0.72, 0.9),
            (0.38, 0.42, 0.95),
            (0.82, 0.3, 0.86),
        ];
        for (index, &(r, g, b)) in colors.iter().enumerate() {
            let start = index as f64 * std::f64::consts::TAU / colors.len() as f64;
            let end = (index + 1) as f64 * std::f64::consts::TAU / colors.len() as f64;
            cr.move_to(x, y);
            cr.arc(x, y, radius, start, end + 0.01);
            cr.close_path();
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill();
        }
    });
    dot
}

pub(super) struct ColorControl {
    pub button: Button,
    pub panel: Revealer,
    color: Rc<Cell<DrawColor>>,
    dot: DrawingArea,
    swatches: Vec<(DrawColor, Button)>,
    custom: Button,
    custom_panel: Revealer,
}

impl ColorControl {
    pub fn new(scale: f64, host: &Rc<EditorHost>, canvas: Option<Weak<Canvas>>) -> Self {
        let color = Rc::new(Cell::new(palette_color(PALETTE[0].1)));
        let dot = color_dot(scale, color.clone());
        let button = Button::new();
        button.add_css_class("image-editor-color-trigger");
        button.set_tooltip_text(Some(&t("Annotation color")));
        button.update_property(&[gtk4::accessible::Property::Label(&t("Annotation color"))]);
        let content = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        content.append(&dot);
        let chevron = gtk4::Image::from_icon_name("chevron-down-regular");
        chevron.set_pixel_size(px(10.0, scale));
        content.append(&chevron);
        button.set_child(Some(&content));

        let panel = Revealer::new();
        panel.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
        panel.set_transition_duration(180);
        panel.set_halign(Align::Center);
        let grid = Grid::new();
        grid.add_css_class("image-editor-color-palette");
        grid.set_column_spacing(px(3.0, scale).max(0) as u32);
        grid.set_row_spacing(px(3.0, scale).max(0) as u32);
        let mut swatches = Vec::new();
        for (index, &(name, rgb)) in PALETTE.iter().enumerate() {
            let value = palette_color(rgb);
            let swatch = Button::new();
            swatch.add_css_class("image-editor-color-swatch");
            swatch.set_tooltip_text(Some(&t(name)));
            swatch.update_property(&[gtk4::accessible::Property::Label(&t(name))]);
            swatch.set_child(Some(&color_dot(scale, Rc::new(Cell::new(value)))));
            swatch.connect_clicked({
                let host = host.clone();
                let panel = panel.downgrade();
                move |_| {
                    apply_color(&host, value);
                    if let Some(panel) = panel.upgrade() {
                        panel.set_reveal_child(false);
                    }
                }
            });
            grid.attach(&swatch, (index % 11) as i32, (index / 11) as i32, 1, 1);
            swatches.push((value, swatch));
        }

        let custom = Button::new();
        custom.add_css_class("image-editor-color-swatch");
        custom.set_tooltip_text(Some(&t("Custom color")));
        custom.update_property(&[gtk4::accessible::Property::Label(&t("Custom color"))]);
        custom.set_child(Some(&rainbow_dot(scale)));
        let editor = Rc::new(CustomColor::new(scale, {
            let host = host.clone();
            Rc::new(move |value| apply_color(&host, value))
        }));
        editor.panel.connect_reveal_child_notify({
            let color = color.clone();
            let custom = custom.downgrade();
            let swatches: Vec<_> = swatches
                .iter()
                .map(|(value, button)| (*value, button.downgrade()))
                .collect();
            move |panel| {
                let Some(custom) = custom.upgrade() else {
                    return;
                };
                let swatches: Vec<_> = swatches
                    .iter()
                    .filter_map(|(value, button)| button.upgrade().map(|b| (*value, b)))
                    .collect();
                sync_selection(color.get(), panel.reveals_child(), &swatches, &custom);
            }
        });
        custom.connect_clicked({
            let color = color.clone();
            let editor = editor.clone();
            let host = Rc::downgrade(host);
            let canvas = canvas.clone();
            let grid = grid.downgrade();
            move |_| {
                if editor.panel.reveals_child() {
                    editor.panel.set_reveal_child(false);
                } else {
                    if let (Some(canvas), Some(host), Some(grid)) = (
                        canvas.as_ref().and_then(Weak::upgrade),
                        host.upgrade(),
                        grid.upgrade(),
                    ) {
                        picker_placement::position(&canvas, &host, &grid, &editor.panel, scale);
                        let canvas = Rc::downgrade(&canvas);
                        let host = Rc::downgrade(&host);
                        let grid = grid.downgrade();
                        let panel = editor.panel.downgrade();
                        let previous = Cell::new(None);
                        if let Some(canvas_now) = canvas.upgrade() {
                            canvas_now.area.add_tick_callback(move |_, _| {
                                let (Some(canvas), Some(host), Some(grid), Some(panel)) = (
                                    canvas.upgrade(),
                                    host.upgrade(),
                                    grid.upgrade(),
                                    panel.upgrade(),
                                ) else {
                                    return gtk4::glib::ControlFlow::Break;
                                };
                                if !panel.reveals_child() {
                                    return gtk4::glib::ControlFlow::Break;
                                }
                                let transform = canvas.view_transform();
                                let anchor = grid.compute_bounds(&canvas.area);
                                let state = anchor.map(|r| {
                                    (
                                        canvas.area.width(),
                                        canvas.area.height(),
                                        transform.scale,
                                        transform.offset_x,
                                        transform.offset_y,
                                        r.x(),
                                        r.y(),
                                        r.width(),
                                        r.height(),
                                    )
                                });
                                if state != previous.get() {
                                    previous.set(state);
                                    picker_placement::position(
                                        &canvas, &host, &grid, &panel, scale,
                                    );
                                }
                                gtk4::glib::ControlFlow::Continue
                            });
                        }
                    }
                    editor.open(color.get());
                }
            }
        });
        grid.attach(&custom, 10, 1, 1, 1);
        let body = GtkBox::new(Orientation::Vertical, 0);
        body.append(&grid);
        if let Some(canvas) = canvas.as_ref().and_then(Weak::upgrade) {
            editor.panel.set_halign(Align::Start);
            editor.panel.set_valign(Align::Start);
            canvas.root.add_overlay(&editor.panel);
        } else {
            body.append(&editor.panel);
        }
        panel.set_child(Some(&body));
        panel.connect_reveal_child_notify({
            let editor_panel = editor.panel.downgrade();
            move |panel| {
                if !panel.reveals_child() {
                    if let Some(editor_panel) = editor_panel.upgrade() {
                        editor_panel.set_reveal_child(false);
                    }
                }
            }
        });
        button.connect_clicked({
            let panel = panel.clone();
            move |_| panel.set_reveal_child(!panel.reveals_child())
        });
        Self {
            button,
            panel,
            color,
            dot,
            swatches,
            custom,
            custom_panel: editor.panel.clone(),
        }
    }

    pub fn sync(&self, st: &EditorState) {
        let color = active_color(st);
        self.color.set(color);
        self.dot.queue_draw();
        sync_selection(
            color,
            self.custom_panel.reveals_child(),
            &self.swatches,
            &self.custom,
        );
    }

    pub fn wrap(&self, bar: &GtkBox) -> GtkBox {
        let root = GtkBox::new(Orientation::Vertical, 0);
        bar.set_halign(Align::Center);
        root.append(bar);
        root.append(&self.panel);
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{AnnotationAction, Point, Rect};
    use image::RgbaImage;

    fn host_with_document() -> Rc<EditorHost> {
        let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
        *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(32, 32)));
        host
    }

    #[test]
    fn palette_has_two_rows_with_one_custom_slot() {
        assert_eq!(PALETTE.len(), 21);
        assert!(PALETTE.iter().all(|(_, rgb)| palette_color(*rgb).a == 1.0));
        assert_eq!(
            draw_color_to_rgba_u8(palette_color(0xff963e)),
            (255, 150, 62, 255)
        );
    }

    #[test]
    fn color_updates_defaults_and_selected_annotations_with_undo() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = host_with_document();
            let original = palette_color(PALETTE[0].1);
            let chosen = DrawColor::new(0.2, 0.4, 0.6, 0.5);
            host.mutate(|st| {
                st.push_action(AnnotationAction::Box {
                    rect: Rect {
                        x: 1,
                        y: 1,
                        width: 8,
                        height: 8,
                    },
                    color: original,
                    stroke_size: 4.0,
                    shadow: false,
                });
                st.selected_action_index = Some(0);
            });
            apply_color(&host, chosen);
            host.read(|st| {
                assert_eq!(st.selected_color, chosen);
                assert_eq!(active_color(st), chosen);
            });
            host.undo();
            host.read(|st| match &st.actions[0] {
                AnnotationAction::Box { color, .. } => assert_eq!(*color, original),
                _ => panic!("expected a box"),
            });
            host.redo();
            host.read(|st| match &st.actions[0] {
                AnnotationAction::Box { color, .. } => assert_eq!(*color, chosen),
                _ => panic!("expected a box"),
            });
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn color_updates_in_progress_text_and_new_annotations() {
        let host = host_with_document();
        let chosen = palette_color(PALETTE[4].1);
        apply_color(&host, chosen);
        host.read(|st| assert_eq!(active_color(st), chosen));
        host.mutate(|st| st.begin_text_input(Point { x: 4.0, y: 4.0 }, 20.0, 20.0));
        let custom = DrawColor::new(0.1, 0.3, 0.8, 0.4);
        apply_color(&host, custom);
        host.read(|st| {
            assert_eq!(active_color(st), custom);
            assert_eq!(st.selected_color, custom);
        });
    }

    #[test]
    fn palette_widgets_toggle_select_and_sync_without_mutating() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = host_with_document();
            host.mutate(|st| st.selected_color = palette_color(PALETTE[0].1));
            let control = ColorControl::new(1.0, &host, None);
            host.read(|st| control.sync(st));
            control.button.emit_clicked();
            assert!(control.panel.reveals_child());
            control.swatches[4].1.emit_clicked();
            assert!(!control.panel.reveals_child());
            host.read(|st| {
                assert_eq!(st.selected_color, palette_color(PALETTE[4].1));
                control.sync(st);
            });
            assert!(control.swatches[4].1.has_css_class("selected"));
            assert!(!control.swatches[0].1.has_css_class("selected"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn custom_editor_expands_in_palette_and_closes_with_it() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = host_with_document();
            host.mutate(|st| st.selected_color = palette_color(PALETTE[0].1));
            let control = ColorControl::new(1.0, &host, None);
            host.read(|st| control.sync(st));
            let body = control.panel.child().unwrap().downcast::<GtkBox>().unwrap();
            let grid = body.first_child().unwrap().downcast::<Grid>().unwrap();
            let custom = grid.child_at(10, 1).unwrap().downcast::<Button>().unwrap();
            let editor = body.last_child().unwrap().downcast::<Revealer>().unwrap();
            control.button.emit_clicked();
            assert!(control.swatches[0].1.has_css_class("selected"));
            custom.emit_clicked();
            assert!(control.panel.reveals_child());
            assert!(editor.reveals_child());
            assert!(custom.has_css_class("selected"));
            assert!(!control.swatches[0].1.has_css_class("selected"));
            control.button.emit_clicked();
            assert!(!control.panel.reveals_child());
            assert!(!editor.reveals_child());
            assert!(control.swatches[0].1.has_css_class("selected"));
            assert!(!custom.has_css_class("selected"));
            control.button.emit_clicked();
            custom.emit_clicked();
            let field = editor
                .child()
                .unwrap()
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .first_child()
                .unwrap();
            let controllers = field.observe_controllers();
            let gesture = (0..controllers.n_items())
                .find_map(|index| {
                    controllers
                        .item(index)?
                        .downcast::<gtk4::GestureDrag>()
                        .ok()
                })
                .unwrap();
            let before = host.read(|st| st.selected_color).unwrap();
            gesture.emit_by_name::<()>("drag-begin", &[&0.0f64, &0.0f64]);
            host.read(|st| {
                assert_ne!(st.selected_color, before);
                control.sync(st);
            });
            assert!(custom.has_css_class("selected"));
            assert!(!control.swatches[0].1.has_css_class("selected"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn custom_picker_uses_canvas_overlay_and_moves_around_drawing() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let canvas = super::super::canvas::build(1.0, true);
            let host = canvas.host.clone();
            let mut state = EditorState::new(RgbaImage::new(1000, 700));
            state.push_action(AnnotationAction::Box {
                rect: Rect {
                    x: 400,
                    y: 70,
                    width: 200,
                    height: 450,
                },
                color: palette_color(PALETTE[0].1),
                stroke_size: 4.0,
                shadow: false,
            });
            *host.editor.borrow_mut() = Some(state);
            canvas.sync_view_to_document();
            let control = ColorControl::new(1.0, &host, Some(Rc::downgrade(&canvas)));
            let bar = GtkBox::new(Orientation::Horizontal, 0);
            bar.append(&control.button);
            let wrapper = control.wrap(&bar);
            wrapper.set_halign(Align::Center);
            wrapper.set_valign(Align::Start);
            wrapper.set_margin_top(50);
            canvas.root.add_overlay(&wrapper);
            assert_eq!(
                control.custom_panel.parent(),
                Some(canvas.root.clone().upcast())
            );
            control.panel.set_transition_duration(0);
            control.panel.set_reveal_child(true);
            canvas.root.measure(Orientation::Horizontal, -1);
            canvas.root.measure(Orientation::Vertical, 1000);
            canvas.root.allocate(1000, 700, -1, None);
            let body = control.panel.child().unwrap().downcast::<GtkBox>().unwrap();
            let grid = body.first_child().unwrap().downcast::<Grid>().unwrap();
            let anchor = grid.compute_bounds(&canvas.area).unwrap();
            let card = control.custom_panel.child().unwrap();
            let (_, width, _, _) = card.measure(Orientation::Horizontal, -1);
            let centered = f64::from(anchor.x() + anchor.width() / 2.0) - f64::from(width) / 2.0;
            control.custom_panel.set_transition_duration(0);
            control.custom.emit_clicked();
            assert!(control.custom_panel.reveals_child());
            assert!((f64::from(control.custom_panel.margin_start()) - centered).abs() > 10.0);
            assert!(control.custom_panel.margin_start() >= 0);
            canvas.root.measure(Orientation::Horizontal, -1);
            canvas.root.measure(Orientation::Vertical, 1000);
            canvas.root.allocate(1000, 700, -1, None);
            let placed = control.custom_panel.compute_bounds(&canvas.area).unwrap();
            assert!((placed.x() - control.custom_panel.margin_start() as f32).abs() <= 1.0);
            assert!((placed.y() - control.custom_panel.margin_top() as f32).abs() <= 1.0);
            assert!(placed.width() > 0.0 && placed.height() > 0.0);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn palette_styles_parse() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let css = include_str!("image_editor.css");
            let start = css.find("button.image-editor-color-trigger").unwrap();
            let end = css.find("/* Menus match the Settings").unwrap();
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
        assert!(errors.is_empty(), "palette CSS errors: {errors:?}");
    }
}
