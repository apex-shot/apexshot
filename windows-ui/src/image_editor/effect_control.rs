//! Contextual intensity control for selected effect regions.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation, Scale};
use std::cell::Cell;
use std::rc::Rc;

use super::annotate::EditorHost;
use super::canvas::Canvas;
use super::effect_mode::active_method;
use super::ui_support::{px, TOP_CHROME_HEIGHT};
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::{
    AnnotationAction, ObfuscateMethod, Point, Rect, Tool, ViewTransform,
};
use crate::i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EffectKind {
    Obfuscate,
    Focus,
}

fn selected_effect(st: &EditorState) -> Option<(EffectKind, Rect, f64)> {
    match (st.selected_tool, st.selected_action()?) {
        (
            Tool::Select | Tool::Obfuscate,
            AnnotationAction::Obfuscate {
                rect,
                method,
                amount,
            },
        ) if *method != ObfuscateMethod::Blackout => Some((EffectKind::Obfuscate, *rect, *amount)),
        (Tool::Select | Tool::Focus, AnnotationAction::Focus { rect, intensity }) => {
            Some((EffectKind::Focus, *rect, *intensity))
        }
        _ => None,
    }
}

fn apply_intensity(st: &mut EditorState, kind: EffectKind, value: f64) -> bool {
    match kind {
        EffectKind::Obfuscate => {
            let method = active_method(st);
            if method == ObfuscateMethod::Blackout {
                return false;
            }
            st.obfuscate_method = method;
            let default_changed = st.set_current_obfuscate_amount_and_check(value);
            let selected_changed = st.set_selected_obfuscate_action_amount_without_rebuild(
                st.current_obfuscate_amount(),
            );
            default_changed || selected_changed
        }
        EffectKind::Focus => {
            let default_changed = st.set_current_focus_intensity_and_check(value);
            let selected_changed = st
                .set_selected_focus_action_intensity_without_rebuild(st.current_focus_intensity());
            default_changed || selected_changed
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn place_slider(
    view: Bounds,
    region: Bounds,
    width: f64,
    height: f64,
    gap: f64,
) -> Option<(f64, f64)> {
    if region.x + region.width <= 0.0
        || region.x >= view.width
        || region.y + region.height <= view.y
        || region.y >= view.y + view.height
        || width > view.width
        || height > view.height
    {
        return None;
    }
    let max_x = (view.width - width).max(0.0);
    let x = (region.x + (region.width - width) / 2.0).clamp(0.0, max_x);
    let top = view.y;
    let bottom = view.y + view.height;
    let above = region.y - gap - height;
    let below = region.y + region.height + gap;
    let y = if above >= top {
        above.min(bottom - height)
    } else if below + height <= bottom {
        below.max(top)
    } else {
        let above = above.clamp(top, bottom - height);
        let below = below.clamp(top, bottom - height);
        let overlap =
            |y: f64| ((y + height).min(region.y + region.height) - y.max(region.y)).max(0.0);
        if overlap(above) <= overlap(below) {
            above
        } else {
            below
        }
    };
    Some((x, y))
}

fn region_in_view(rect: Rect, transform: ViewTransform) -> Bounds {
    let point = transform.image_to_view(Point {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
    });
    Bounds {
        x: point.x,
        y: point.y,
        width: f64::from(rect.width) * transform.scale,
        height: f64::from(rect.height) * transform.scale,
    }
}

pub(super) struct EffectControl {
    pub root: GtkBox,
    slider: Scale,
    value: Label,
    syncing: Rc<Cell<bool>>,
    kind: Rc<Cell<EffectKind>>,
    scale: f64,
}

impl EffectControl {
    pub fn new(scale: f64, host: &Rc<EditorHost>) -> Self {
        let root = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
        root.add_css_class("image-editor-effect-control");
        root.set_halign(Align::Start);
        root.set_valign(Align::Start);
        root.set_visible(false);
        let slider = Scale::with_range(Orientation::Horizontal, 1.0, 25.0, 1.0);
        slider.add_css_class("image-editor-effect-slider");
        slider.set_draw_value(false);
        slider.set_size_request(px(140.0, scale), -1);
        slider.set_valign(Align::Center);
        let value = Label::new(None);
        value.add_css_class("image-editor-effect-value");
        value.set_width_chars(4);
        root.append(&slider);
        root.append(&value);
        let syncing = Rc::new(Cell::new(false));
        let kind = Rc::new(Cell::new(EffectKind::Obfuscate));
        slider.connect_value_changed({
            let host = host.clone();
            let syncing = syncing.clone();
            let kind = kind.clone();
            move |slider| {
                if !syncing.get() {
                    host.mutate_effects(|st| apply_intensity(st, kind.get(), slider.value()));
                }
            }
        });
        Self {
            root,
            slider,
            value,
            syncing,
            kind,
            scale,
        }
    }

    pub fn sync(&self, st: &EditorState, canvas: &Canvas) {
        let Some((kind, rect, value)) = selected_effect(st) else {
            self.root.set_visible(false);
            return;
        };
        let width = f64::from(canvas.area.width());
        let height = f64::from(canvas.area.height());
        if width <= 1.0 || height <= 1.0 {
            self.root.set_visible(false);
            return;
        }
        self.syncing.set(true);
        self.kind.set(kind);
        let (min, max, label) = match kind {
            EffectKind::Obfuscate => (1.0, 25.0, t("Obfuscate intensity")),
            EffectKind::Focus => (10.0, 90.0, t("Focus background intensity")),
        };
        self.slider.set_range(min, max);
        self.slider.set_value(value.clamp(min, max));
        self.slider.set_tooltip_text(Some(&label));
        self.slider
            .update_property(&[gtk4::accessible::Property::Label(&label)]);
        self.value.set_text(&if kind == EffectKind::Focus {
            format!("{value:.0}%")
        } else {
            format!("{value:.0}")
        });
        self.syncing.set(false);
        self.root.set_visible(true);
        let (_, panel_width, _, _) = self.root.measure(Orientation::Horizontal, -1);
        let (_, panel_height, _, _) = self.root.measure(Orientation::Vertical, panel_width);
        let panel_width = (panel_width - self.root.margin_start() - self.root.margin_end()).max(0);
        let panel_height =
            (panel_height - self.root.margin_top() - self.root.margin_bottom()).max(0);
        let safe_top =
            (TOP_CHROME_HEIGHT * 2.0 * self.scale).min((height - f64::from(panel_height)).max(0.0));
        let viewport = Bounds {
            x: 0.0,
            y: safe_top,
            width,
            height: (height - safe_top - 8.0 * self.scale).max(0.0),
        };
        let position = place_slider(
            viewport,
            region_in_view(rect, canvas.view_transform()),
            f64::from(panel_width),
            f64::from(panel_height),
            8.0 * self.scale,
        );
        if let Some((x, y)) = position {
            let (x, y) = (x.round() as i32, y.round() as i32);
            if self.root.margin_start() != x {
                self.root.set_margin_start(x);
            }
            if self.root.margin_top() != y {
                self.root.set_margin_top(y);
            }
            self.root.set_visible(true);
        } else {
            self.root.set_visible(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn positions_above_or_below_and_clamps_to_viewport() {
        let viewport = bounds(0.0, 100.0, 800.0, 500.0);
        assert_eq!(
            place_slider(
                viewport,
                bounds(300.0, 300.0, 100.0, 80.0),
                160.0,
                40.0,
                8.0
            ),
            Some((270.0, 252.0))
        );
        assert_eq!(
            place_slider(viewport, bounds(10.0, 105.0, 100.0, 80.0), 160.0, 40.0, 8.0),
            Some((0.0, 193.0))
        );
        assert_eq!(
            place_slider(viewport, bounds(740.0, 550.0, 60.0, 50.0), 160.0, 40.0, 8.0),
            Some((640.0, 502.0))
        );
        assert_eq!(
            place_slider(
                viewport,
                bounds(-200.0, 300.0, 50.0, 80.0),
                160.0,
                40.0,
                8.0
            ),
            None
        );
        assert_eq!(
            place_slider(
                viewport,
                bounds(100.0, 90.0, 200.0, 520.0),
                160.0,
                40.0,
                8.0
            ),
            Some((120.0, 100.0))
        );
    }

    #[test]
    fn region_position_tracks_pan_and_zoom() {
        let mut transform = ViewTransform::for_image(800.0, 600.0);
        transform.scale = 1.5;
        transform.offset_x = -20.0;
        transform.offset_y = 40.0;
        let rect = Rect {
            x: 100,
            y: 20,
            width: 40,
            height: 10,
        };
        assert_eq!(
            region_in_view(rect, transform),
            bounds(130.0, 70.0, 60.0, 15.0)
        );
    }

    #[test]
    fn selected_effect_intensity_updates_and_undoes_even_when_default_matches() {
        let mut st = EditorState::new(RgbaImage::new(32, 32));
        let rect = Rect {
            x: 2,
            y: 2,
            width: 12,
            height: 12,
        };
        st.selected_tool = Tool::Select;
        st.obfuscate_method = ObfuscateMethod::Blackout;
        st.obfuscate_blur_amount = 12.0;
        st.push_action(AnnotationAction::Obfuscate {
            rect,
            method: ObfuscateMethod::Blur,
            amount: 5.0,
        });
        assert_eq!(
            selected_effect(&st),
            Some((EffectKind::Obfuscate, rect, 5.0))
        );
        assert!(apply_intensity(&mut st, EffectKind::Obfuscate, 12.0));
        assert_eq!(
            selected_effect(&st),
            Some((EffectKind::Obfuscate, rect, 12.0))
        );
        assert!(st.undo());
        st.selected_action_index = Some(0);
        assert_eq!(
            selected_effect(&st),
            Some((EffectKind::Obfuscate, rect, 5.0))
        );

        st.push_action(AnnotationAction::Focus {
            rect,
            intensity: 35.0,
        });
        st.focus_intensity = 70.0;
        assert!(apply_intensity(&mut st, EffectKind::Focus, 70.0));
        assert_eq!(selected_effect(&st), Some((EffectKind::Focus, rect, 70.0)));
        assert!(st.undo());
        st.selected_action_index = Some(1);
        assert_eq!(selected_effect(&st), Some((EffectKind::Focus, rect, 35.0)));
    }

    #[test]
    fn blackout_and_incompatible_selection_hide_the_control() {
        let mut st = EditorState::new(RgbaImage::new(32, 32));
        st.selected_tool = Tool::Obfuscate;
        let rect = Rect {
            x: 2,
            y: 2,
            width: 12,
            height: 12,
        };
        st.push_action(AnnotationAction::Obfuscate {
            rect,
            method: ObfuscateMethod::Blackout,
            amount: 0.0,
        });
        assert_eq!(selected_effect(&st), None);
        st.set_obfuscate_method(ObfuscateMethod::Pixelate);
        assert!(selected_effect(&st).is_some());
        st.selected_tool = Tool::Pen;
        assert_eq!(selected_effect(&st), None);
        st.clear_selection();
        assert_eq!(selected_effect(&st), None);
    }

    #[test]
    fn contextual_slider_css_parses() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let css = include_str!("image_editor.css");
            let start = css.find(".image-editor-effect-control {").unwrap();
            let end = css.find(".image-editor-history-pill {").unwrap();
            let provider = gtk4::CssProvider::new();
            let errors = Rc::new(std::cell::RefCell::new(Vec::new()));
            let captured = errors.clone();
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string())
            });
            provider.load_from_data(&css[start..end]);
            let result = errors.borrow().clone();
            result
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
        assert!(errors.is_empty(), "effect slider CSS errors: {errors:?}");
    }

    #[test]
    fn slider_size_measurement_excludes_position_margins() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            let control = EffectControl::new(1.0, &host);
            control.root.set_visible(true);
            let (_, width, _, _) = control.root.measure(Orientation::Horizontal, -1);
            let (_, height, _, _) = control.root.measure(Orientation::Vertical, width);
            control.root.set_margin_start(100);
            control.root.set_margin_top(150);
            let (_, shifted_width, _, _) = control.root.measure(Orientation::Horizontal, -1);
            let (_, shifted_height, _, _) =
                control.root.measure(Orientation::Vertical, shifted_width);
            assert_eq!(shifted_width - 100, width);
            assert_eq!(shifted_height - 150, height);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn overlay_stays_anchored_and_updates_selected_region() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let canvas = super::super::canvas::build(1.0, true);
            let control = EffectControl::new(1.0, &canvas.host);
            canvas.root.add_overlay(&control.root);
            let window = gtk4::Window::new();
            window.set_child(Some(&canvas.root));
            gtk4::prelude::WidgetExt::realize(&window);
            canvas.root.measure(Orientation::Horizontal, -1);
            canvas.root.measure(Orientation::Vertical, 800);
            canvas.root.allocate(800, 600, -1, None);
            canvas.area.measure(Orientation::Horizontal, -1);
            canvas.area.measure(Orientation::Vertical, 800);
            canvas.area.allocate(800, 600, -1, None);
            assert!(
                canvas.area.width() > 1 && canvas.area.height() > 1,
                "area dimensions: {}x{}",
                canvas.area.width(),
                canvas.area.height()
            );
            let mut st = EditorState::new(RgbaImage::new(400, 300));
            st.selected_tool = Tool::Focus;
            st.push_action(AnnotationAction::Focus {
                rect: Rect {
                    x: 100,
                    y: 100,
                    width: 80,
                    height: 60,
                },
                intensity: 45.0,
            });
            *canvas.host.editor.borrow_mut() = Some(st);
            canvas.sync_view_to_document();
            canvas.host.read(|st| control.sync(st, &canvas));
            assert!(control.root.get_visible());
            let position = (control.root.margin_start(), control.root.margin_top());
            assert!(position.0 > 0 && position.1 > 0);
            canvas.host.read(|st| control.sync(st, &canvas));
            assert_eq!(
                (control.root.margin_start(), control.root.margin_top()),
                position
            );
            control.slider.set_value(73.0);
            canvas
                .host
                .read(|st| assert_eq!(st.selected_focus_action_intensity(), Some(73.0)));
            canvas.host.mutate(|st| st.clear_selection());
            canvas.host.read(|st| control.sync(st, &canvas));
            assert!(!control.root.get_visible());
            window.set_child(None::<&gtk4::Widget>);
            window.destroy();
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
