use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, DrawingArea, Grid, Orientation, Revealer};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::custom_color::CustomColor;
use super::picker_placement::{self, PickerPlacementContext};
use super::toolbar_controls::{chevron, MenuTracker};
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

/// Which palette entry a colour is, exactly: the index it matches, or `None`
/// for a colour the palette does not carry.
///
/// Exact rather than nearest: a near miss is a custom colour, and the picker has
/// to light the rainbow tile rather than a swatch that merely looks close.
fn preset_index(color: DrawColor, mut palette: impl Iterator<Item = DrawColor>) -> Option<usize> {
    let rgba = draw_color_to_rgba_u8(color);
    palette.position(|value| draw_color_to_rgba_u8(value) == rgba)
}

fn sync_selection(
    color: DrawColor,
    custom_open: bool,
    swatches: &[(DrawColor, Button)],
    custom: &Button,
) {
    let preset = preset_index(color, swatches.iter().map(|(value, _)| *value));
    for (index, (_, button)) in swatches.iter().enumerate() {
        if !custom_open && preset == Some(index) {
            button.add_css_class("selected");
        } else {
            button.remove_css_class("selected");
        }
    }
    if custom_open || preset.is_none() {
        custom.add_css_class("selected");
    } else {
        custom.remove_css_class("selected");
    }
}

fn color_dot(color: Rc<Cell<DrawColor>>) -> DrawingArea {
    let dot = DrawingArea::new();
    dot.set_content_width(20);
    dot.set_content_height(20);
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

fn rainbow_dot() -> DrawingArea {
    let dot = DrawingArea::new();
    dot.set_content_width(20);
    dot.set_content_height(20);
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
    pub(super) color: Rc<Cell<DrawColor>>,
    dot: DrawingArea,
    pub(super) swatches: Vec<(DrawColor, Button)>,
    pub(super) custom: Button,
    pub(super) custom_panel: Revealer,
}

impl ColorControl {
    pub fn new(
        _state: &Arc<Mutex<EditorState>>,
        apply: Rc<dyn Fn(DrawColor)>,
        menus: &MenuTracker,
        placement: Option<Rc<PickerPlacementContext>>,
    ) -> Self {
        let color = Rc::new(Cell::new(palette_color(PALETTE[0].1)));
        let dot = color_dot(color.clone());
        let button = Button::new();
        button.set_focusable(false);
        button.add_css_class("editor-color-trigger");
        button.set_tooltip_text(Some(&t("Annotation color")));
        button.update_property(&[gtk4::accessible::Property::Label(&t("Annotation color"))]);
        let content = GtkBox::new(Orientation::Horizontal, 6);
        content.append(&dot);
        let arrow = chevron();
        content.append(&arrow);
        button.set_child(Some(&content));

        let panel = Revealer::new();
        panel.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
        panel.set_transition_duration(180);
        panel.set_halign(Align::Center);
        let grid = Grid::new();
        grid.add_css_class("editor-color-palette");
        grid.set_column_spacing(3);
        grid.set_row_spacing(3);
        let mut swatches = Vec::new();
        for (index, &(name, rgb)) in PALETTE.iter().enumerate() {
            let value = palette_color(rgb);
            let swatch = Button::new();
            swatch.set_focusable(false);
            swatch.add_css_class("editor-color-swatch");
            swatch.set_tooltip_text(Some(&t(name)));
            swatch.update_property(&[gtk4::accessible::Property::Label(&t(name))]);
            swatch.set_child(Some(&color_dot(Rc::new(Cell::new(value)))));
            swatch.connect_clicked({
                let apply = apply.clone();
                let panel = panel.downgrade();
                move |_| {
                    apply(value);
                    if let Some(panel) = panel.upgrade() {
                        panel.set_reveal_child(false);
                    }
                }
            });
            grid.attach(&swatch, (index % 11) as i32, (index / 11) as i32, 1, 1);
            swatches.push((value, swatch));
        }

        let custom = Button::new();
        custom.set_focusable(false);
        custom.add_css_class("editor-color-swatch");
        custom.set_tooltip_text(Some(&t("Custom color")));
        custom.update_property(&[gtk4::accessible::Property::Label(&t("Custom color"))]);
        custom.set_child(Some(&rainbow_dot()));
        let editor = Rc::new(CustomColor::new(apply));
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
            let placement = placement.clone();
            let grid = grid.downgrade();
            move |_| {
                if editor.panel.reveals_child() {
                    editor.panel.set_reveal_child(false);
                } else {
                    if let (Some(context), Some(grid)) = (placement.clone(), grid.upgrade()) {
                        picker_placement::position(&context, &grid, &editor.panel);
                        let grid = grid.downgrade();
                        let panel = editor.panel.downgrade();
                        let context = context.clone();
                        if let Some(area_now) = context.drawing_area.upgrade() {
                            area_now.add_tick_callback(move |_, _| {
                                let (Some(grid), Some(panel)) = (grid.upgrade(), panel.upgrade())
                                else {
                                    return gtk4::glib::ControlFlow::Break;
                                };
                                if !panel.reveals_child() {
                                    return gtk4::glib::ControlFlow::Break;
                                }
                                picker_placement::position(&context, &grid, &panel);
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
        if let Some(overlay) = placement
            .as_ref()
            .and_then(|context| context.overlay.upgrade())
        {
            editor.panel.set_halign(Align::Start);
            editor.panel.set_valign(Align::Start);
            overlay.add_overlay(&editor.panel);
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
            let menus = menus.clone();
            move |_| {
                if panel.reveals_child() {
                    panel.set_reveal_child(false);
                } else {
                    menus.close_all();
                    panel.set_reveal_child(true);
                }
            }
        });
        menus.register_palette(&panel);
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
        root.set_halign(Align::Start);
        root.set_valign(Align::Start);
        bar.set_halign(Align::Center);
        root.append(bar);
        root.append(&self.panel);
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn harness() -> (
        ColorControl,
        Rc<RefCell<Vec<DrawColor>>>,
        MenuTracker,
        Arc<Mutex<EditorState>>,
    ) {
        let state = Arc::new(Mutex::new(EditorState::new(image::RgbaImage::new(32, 32))));
        let applied = Rc::new(RefCell::new(Vec::new()));
        let menus = MenuTracker::default();
        let control = ColorControl::new(
            &state,
            {
                let applied = applied.clone();
                Rc::new(move |color| applied.borrow_mut().push(color))
            },
            &menus,
            None,
        );
        (control, applied, menus, state)
    }

    #[test]
    fn trigger_toggles_only_the_circle_palette() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (control, applied, _, _) = harness();
            assert_eq!(control.swatches.len(), 21);
            assert!(!control.panel.reveals_child());
            assert!(!control.custom_panel.reveals_child());
            control.button.emit_clicked();
            assert!(control.panel.reveals_child());
            assert!(!control.custom_panel.reveals_child());
            assert!(applied.borrow().is_empty());
            control.button.emit_clicked();
            assert!(!control.panel.reveals_child());
        }) else {
            return;
        };
    }

    #[test]
    fn swatch_applies_and_closes_rainbow_opens_custom_and_folding_cascades() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (control, applied, menus, _) = harness();
            control.button.emit_clicked();
            control.swatches[2].1.emit_clicked();
            assert_eq!(applied.borrow().len(), 1);
            assert_eq!(
                draw_color_to_rgba_u8(applied.borrow()[0]),
                (0xf4, 0x3d, 0x2b, 255)
            );
            assert!(!control.panel.reveals_child());

            control.button.emit_clicked();
            control.custom.emit_clicked();
            assert!(control.custom_panel.reveals_child());
            assert!(control.panel.reveals_child());
            assert_eq!(applied.borrow().len(), 1, "opening custom must not apply");

            control.panel.set_reveal_child(false);
            assert!(!control.custom_panel.reveals_child());

            control.button.emit_clicked();
            menus.close_all();
            assert!(!control.panel.reveals_child());
            assert!(!menus.any_open());
        }) else {
            return;
        };
    }

    #[test]
    fn wrapped_bar_does_not_fill_the_canvas_overlay() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (control, _, _, _) = harness();
            let bar = GtkBox::new(Orientation::Horizontal, 0);
            let root = control.wrap(&bar);
            assert_eq!(root.halign(), Align::Start);
            assert_eq!(root.valign(), Align::Start);
            assert_eq!(bar.halign(), Align::Center);
        }) else {
            return;
        };
    }

    /// The palette decision, without a toolkit.
    ///
    /// `sync_marks_the_preset_ring_and_rainbow_for_off_palette_colours` covers
    /// the wiring from that decision to the `selected` class; what the decision
    /// itself has to get right is the exact match, including the last palette
    /// entry and the case where a colour is only *near* one.
    #[test]
    fn preset_index_matches_exact_palette_colours_and_rejects_near_misses() {
        let palette = || PALETTE.iter().map(|&(_, rgb)| palette_color(rgb));
        for (index, &(name, rgb)) in PALETTE.iter().enumerate() {
            assert_eq!(
                preset_index(palette_color(rgb), palette()),
                Some(index),
                "{name} must resolve to its own swatch"
            );
        }
        // A colour a few points off Blue is not Blue: it opens as custom.
        assert_eq!(
            preset_index(DrawColor::new(0.13, 0.52, 0.77, 1.0), palette()),
            None
        );
    }

    #[test]
    fn sync_marks_the_preset_ring_and_rainbow_for_off_palette_colours() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (control, _, _, state) = harness();
            state.lock().unwrap().selected_color = palette_color(0x39a7ef);
            control.sync(&state.lock().unwrap());
            assert!(control.swatches[7].1.has_css_class("selected"));
            assert!(!control.custom.has_css_class("selected"));

            state.lock().unwrap().selected_color = DrawColor::new(0.13, 0.52, 0.77, 1.0);
            control.sync(&state.lock().unwrap());
            assert!(control.custom.has_css_class("selected"));
            assert!(!control.swatches[7].1.has_css_class("selected"));
        }) else {
            return;
        };
    }
}
