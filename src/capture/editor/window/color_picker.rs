use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, CssProvider, Image, Popover};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::i18n::t;

use super::super::color::{
    draw_color_to_rgba_u8, load_persisted_custom_slot_colors, DEFAULT_COLOR_INDEX,
};
use super::super::state::EditorState;
use super::super::types::{BackgroundStyle, DrawColor, Point, Tool};
use super::super::ui_support::color_swatch_button;
use crate::capture::editor::window::icon_names;
use crate::recording::editor::window::custom_wallpaper_popover::{
    build_color_picker as build_shared_color_picker, build_color_popover,
};

pub struct ColorPickerParts {
    pub popover: Popover,
    pub color_buttons: Vec<Button>,
    pub color_picker_dot: GtkBox,
    pub color_class_names: Vec<&'static str>,
    pub eyedropper_btn: Button,
    pub sync_for_active_tool: Rc<dyn Fn()>,
    pub sync_picker_from_color: Rc<dyn Fn(DrawColor)>,
    pub apply_picker_color: Rc<dyn Fn(DrawColor)>,
    pub set_picker_panel_visibility: Rc<dyn Fn(bool)>,
    pub custom_slot_colors: Rc<RefCell<Vec<Option<DrawColor>>>>,
    pub refresh_custom_color_slots: Rc<dyn Fn()>,
    pub register_external_sync: Rc<dyn Fn(Rc<dyn Fn()>)>,
}

pub fn build_color_picker(
    state: Arc<Mutex<EditorState>>,
    canvas_queue_draw_signal: Rc<dyn Fn()>,
    _show_color_names: bool,
    set_background_fill: Rc<RefCell<Option<Rc<dyn Fn(DrawColor)>>>>,
) -> ColorPickerParts {
    let color_specs = [
        ("Black", "editor-color-black"),
        ("Blue", "editor-color-blue"),
        ("Dark Green", "editor-color-dark-green"),
        ("Red", "editor-color-red"),
        ("Orange", "editor-color-orange"),
        ("Yellow", "editor-color-yellow"),
        ("Green", "editor-color-green"),
        ("Cyan", "editor-color-cyan"),
        ("Blue Bright", "editor-color-blue-bright"),
        ("Purple", "editor-color-purple"),
        ("Pink", "editor-color-pink"),
        ("White", "editor-color-white"),
    ];
    let visible_color_specs = &color_specs[..10];
    let color_class_names: Vec<&'static str> = color_specs
        .iter()
        .map(|(_, class_name)| *class_name)
        .collect();
    let color_buttons: Vec<Button> = visible_color_specs
        .iter()
        .map(|(tooltip, class_name)| color_swatch_button(class_name, tooltip))
        .collect();
    let color_picker_dot = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    color_picker_dot.set_size_request(20, 20);
    color_picker_dot.add_css_class("editor-color-trigger-dot");
    color_picker_dot.add_css_class(color_specs[DEFAULT_COLOR_INDEX].1);
    color_picker_dot.set_widget_name("editor-color-trigger-dot");

    let trigger_dot_css = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &trigger_dot_css,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
    let set_trigger_dot_exact_color: Rc<dyn Fn(DrawColor)> = Rc::new({
        let trigger_dot_css = trigger_dot_css.clone();
        move |color| {
            let (r, g, b, _) = super::super::color::draw_color_to_rgba_u8(color);
            trigger_dot_css.load_from_data(&format!(
                "#editor-color-trigger-dot {{ background: rgba({r}, {g}, {b}, {:.3}); background-image: none; }}",
                color.a.clamp(0.0, 1.0)
            ));
        }
    });
    let clear_trigger_dot_exact_color: Rc<dyn Fn()> = Rc::new({
        let trigger_dot_css = trigger_dot_css.clone();
        move || trigger_dot_css.load_from_data("")
    });

    let custom_slot_colors = Rc::new(RefCell::new(load_persisted_custom_slot_colors(
        color_buttons.len(),
    )));
    let refresh_custom_color_slots: Rc<dyn Fn()> = Rc::new(|| {});
    let current_color = Rc::new(Cell::new(active_tool_color(&state.lock().unwrap())));
    let external_sync = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));

    let apply_picker_color_to_editor: Rc<dyn Fn(DrawColor)> = Rc::new({
        let state = state.clone();
        let color_buttons = color_buttons.clone();
        let color_picker_dot = color_picker_dot.clone();
        let color_class_names = color_class_names.clone();
        let external_sync = external_sync.clone();
        let current_color = current_color.clone();
        let set_trigger_dot_exact_color = set_trigger_dot_exact_color.clone();
        let set_background_fill = set_background_fill.clone();
        let canvas_queue_draw_signal = canvas_queue_draw_signal.clone();
        move |color| {
            current_color.set(color);
            let background_fill = {
                let mut state = state.lock().unwrap();
                let background_fill = (state.selected_tool == Tool::Background).then_some(color);
                if background_fill.is_none() {
                    state.selected_color = color;
                    if state.active_text_input.is_some() {
                        let _ = state.set_selected_action_color(color);
                    }
                }
                background_fill
            };
            if let Some(fill_color) = background_fill {
                if let Some(setter) = set_background_fill.borrow().as_ref() {
                    setter(fill_color);
                } else {
                    state.lock().unwrap().background_style =
                        BackgroundStyle::PlainColor(fill_color);
                }
            }

            let nearest_index = super::super::color::palette_index_for_color(color);
            clear_active_color_picker_palette_state(&color_buttons);
            set_color_picker_trigger_dot_state(
                &color_picker_dot,
                &color_class_names,
                nearest_index,
            );
            set_trigger_dot_exact_color(color);

            canvas_queue_draw_signal();
            if let Some(sync) = external_sync.borrow().as_ref() {
                sync();
            }
        }
    });

    let picker_repaint: Rc<RefCell<Option<std::rc::Weak<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let picker = build_shared_color_picker(
        {
            let current_color = current_color.clone();
            Rc::new(move || {
                let (r, g, b, _) = draw_color_to_rgba_u8(current_color.get());
                (r, g, b)
            })
        },
        {
            let current_color = current_color.clone();
            Rc::new(move |rgb| current_color.set(with_rgb(current_color.get(), rgb)))
        },
        {
            let current_color = current_color.clone();
            let apply_picker_color_to_editor = apply_picker_color_to_editor.clone();
            let picker_repaint = picker_repaint.clone();
            Rc::new(move || {
                apply_picker_color_to_editor(current_color.get());
                if let Some(repaint) = picker_repaint
                    .borrow()
                    .as_ref()
                    .and_then(std::rc::Weak::upgrade)
                {
                    repaint();
                }
            })
        },
    );
    *picker_repaint.borrow_mut() = Some(Rc::downgrade(&picker.repaint));
    let eyedropper_btn = Button::new();
    eyedropper_btn.set_has_frame(false);
    eyedropper_btn.set_focusable(false);
    eyedropper_btn.set_tooltip_text(Some(&t("Pick from screen")));
    eyedropper_btn.add_css_class("editor-eyedropper-button");
    let eyedropper_icon = Image::from_icon_name(icon_names::custom::PIPETTE_SYMBOLIC);
    eyedropper_icon.set_pixel_size(15);
    eyedropper_btn.set_child(Some(&eyedropper_icon));
    let popover = build_color_popover(&t("Color picker"), &picker, Some(&eyedropper_btn));

    let sync_picker_from_color: Rc<dyn Fn(DrawColor)> = Rc::new({
        let current_color = current_color.clone();
        let picker_repaint = picker_repaint.clone();
        move |color| {
            current_color.set(color);
            if let Some(repaint) = picker_repaint
                .borrow()
                .as_ref()
                .and_then(std::rc::Weak::upgrade)
            {
                repaint();
            }
        }
    });

    let sync_picker_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let color_buttons = color_buttons.clone();
        let color_picker_dot = color_picker_dot.clone();
        let color_class_names = color_class_names.clone();
        let sync_picker_from_color = sync_picker_from_color.clone();
        let set_trigger_dot_exact_color = set_trigger_dot_exact_color.clone();
        let clear_trigger_dot_exact_color = clear_trigger_dot_exact_color.clone();
        move || {
            let state = state.lock().unwrap();
            let color = active_tool_color(&state);
            let show_palette_state = state.selected_tool != Tool::Background
                || matches!(&state.background_style, BackgroundStyle::PlainColor(_));
            drop(state);
            sync_picker_from_color(color);
            clear_active_color_picker_palette_state(&color_buttons);
            if show_palette_state {
                set_color_picker_trigger_dot_state(
                    &color_picker_dot,
                    &color_class_names,
                    super::super::color::palette_index_for_color(color),
                );
                set_trigger_dot_exact_color(color);
            } else {
                clear_color_picker_trigger_dot_state(&color_picker_dot, &color_class_names);
                clear_trigger_dot_exact_color();
            }
        }
    });
    sync_picker_for_active_tool();

    let register_external_sync: Rc<dyn Fn(Rc<dyn Fn()>)> = Rc::new({
        let external_sync = external_sync.clone();
        move |sync| *external_sync.borrow_mut() = Some(sync)
    });
    let set_picker_panel_visibility: Rc<dyn Fn(bool)> = {
        let popover = popover.downgrade();
        Rc::new(move |show| {
            if let Some(popover) = popover.upgrade() {
                if show {
                    popover.popup();
                } else {
                    popover.popdown();
                }
            }
        })
    };

    ColorPickerParts {
        popover,
        color_buttons,
        color_picker_dot,
        color_class_names,
        eyedropper_btn,
        sync_for_active_tool: sync_picker_for_active_tool,
        sync_picker_from_color,
        apply_picker_color: apply_picker_color_to_editor,
        set_picker_panel_visibility,
        custom_slot_colors,
        refresh_custom_color_slots,
        register_external_sync,
    }
}

fn active_tool_color(state: &EditorState) -> DrawColor {
    if state.selected_tool == Tool::Background {
        if let BackgroundStyle::PlainColor(color) = &state.background_style {
            return *color;
        }
    }
    state.selected_color
}

fn with_rgb(color: DrawColor, rgb: (u8, u8, u8)) -> DrawColor {
    DrawColor::new(
        f64::from(rgb.0) / 255.0,
        f64::from(rgb.1) / 255.0,
        f64::from(rgb.2) / 255.0,
        color.a,
    )
}

pub fn set_active_color_picker_state(
    color_buttons: &[Button],
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    super::super::ui_support::set_active_color_button(color_buttons, active_index);
    set_color_picker_trigger_dot_state(trigger_dot, color_classes, active_index);
}

pub fn clear_active_color_picker_palette_state(color_buttons: &[Button]) {
    for button in color_buttons {
        button.remove_css_class("active-color");
    }
}

pub fn clear_color_picker_trigger_dot_state(trigger_dot: &GtkBox, color_classes: &[&str]) {
    for class_name in color_classes {
        trigger_dot.remove_css_class(class_name);
    }
}

pub fn set_color_picker_trigger_dot_state(
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    clear_color_picker_trigger_dot_state(trigger_dot, color_classes);

    if let Some(class_name) = color_classes.get(active_index) {
        trigger_dot.add_css_class(class_name);
    }
}

pub fn activate_eyedropper(
    popover: &Popover,
    state: Arc<Mutex<EditorState>>,
    eyedropper_mode: Rc<Cell<bool>>,
    eyedropper_point: Rc<RefCell<Option<Point>>>,
    eyedropper_rendered: Rc<RefCell<Option<RgbaImage>>>,
    canvas_eyedropper_ring: &gtk4::DrawingArea,
    drawing_area: &gtk4::DrawingArea,
    set_cursor_crosshair: Rc<dyn Fn()>,
) {
    popover.popdown();
    eyedropper_mode.set(true);
    *eyedropper_point.borrow_mut() = None;
    *eyedropper_rendered.borrow_mut() = state.lock().unwrap().to_rendered_image().ok();
    canvas_eyedropper_ring.set_visible(false);
    canvas_eyedropper_ring.queue_draw();
    set_cursor_crosshair();
    drawing_area.queue_draw();
}

pub fn connect_eyedropper_activation(
    eyedropper_btn: &Button,
    popover: &Popover,
    state: Arc<Mutex<EditorState>>,
    eyedropper_mode: Rc<Cell<bool>>,
    eyedropper_point: Rc<RefCell<Option<Point>>>,
    eyedropper_rendered: Rc<RefCell<Option<RgbaImage>>>,
    canvas_eyedropper_ring: &gtk4::DrawingArea,
    drawing_area: &gtk4::DrawingArea,
    set_cursor_crosshair: Rc<dyn Fn()>,
) {
    let popover = popover.downgrade();
    let canvas_eyedropper_ring = canvas_eyedropper_ring.clone();
    let drawing_area = drawing_area.clone();
    eyedropper_btn.connect_clicked(move |_| {
        let Some(popover) = popover.upgrade() else {
            return;
        };
        activate_eyedropper(
            &popover,
            state.clone(),
            eyedropper_mode.clone(),
            eyedropper_point.clone(),
            eyedropper_rendered.clone(),
            &canvas_eyedropper_ring,
            &drawing_area,
            set_cursor_crosshair.clone(),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::{active_tool_color, with_rgb};
    use crate::capture::editor::state::EditorState;
    use crate::capture::editor::types::{BackgroundStyle, DrawColor, Tool};

    #[test]
    fn rgb_edits_preserve_alpha() {
        for alpha in [0.0, 0.37, 1.0] {
            let color = DrawColor::new(0.1, 0.2, 0.3, alpha);
            let updated = with_rgb(color, (200, 100, 50));
            assert_eq!(
                updated,
                DrawColor::new(200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0, alpha)
            );
        }
    }

    #[test]
    fn picker_reads_the_active_tools_color_including_background_alpha() {
        let mut state = EditorState::new(image::RgbaImage::new(2, 2));
        state.selected_color = DrawColor::new(0.1, 0.2, 0.3, 0.4);
        let background = DrawColor::new(0.9, 0.8, 0.7, 0.6);
        state.background_style = BackgroundStyle::PlainColor(background);
        for tool in [Tool::Pen, Tool::Text, Tool::Highlighter, Tool::Number] {
            state.selected_tool = tool;
            assert_eq!(active_tool_color(&state), state.selected_color);
        }
        state.selected_tool = Tool::Background;
        assert_eq!(active_tool_color(&state), background);
        state.background_style = BackgroundStyle::None;
        assert_eq!(active_tool_color(&state), state.selected_color);
    }

    #[test]
    fn picker_color_edits_do_not_steal_focus_from_the_popover() {
        let source = include_str!("color_picker.rs");
        let apply = source
            .split("let apply_picker_color_to_editor: Rc<dyn Fn(DrawColor)>")
            .nth(1)
            .unwrap()
            .split("let picker_repaint:")
            .next()
            .unwrap();
        assert!(apply.contains("state.set_selected_action_color(color)"));
        assert!(!apply.contains("grab_focus"));
    }
}
