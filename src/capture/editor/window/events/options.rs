//! Tool option callbacks for inspector/toolbar controls (PR 10.10).
//!
//! Owns weight, style, direction, numbering, palette, size-slider, and
//! obfuscation option wiring. Domain widgets are passed via
//! [`ToolOptionsParts`] rather than the full `EventContext`.
//!
//! Text size/font list *sync* used by canvas click re-edit stays in `mod.rs`
//! with the click handlers. Canvas drag/click/keyboard stay separate.

use gtk4::{
    prelude::*, ApplicationWindow, Box as GtkBox, Button, CheckButton, DrawingArea, Entry, Image,
    Popover, Scale,
};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    color::DRAW_COLORS,
    numbering_style::{NumberSize, NumberingStyle},
    pen_weight::{HighlighterMode, PenWeight},
    state::EditorState,
    types::{ArrowStyle, DrawColor, ObfuscateMethod, Tool},
    ui_support::{arrow_style_toolbar_icon, set_button_tool_icon, toolbar_icon_size},
};

use super::super::{
    color_picker,
    cursor::{set_highlighter_cursor, update_pen_cursor, DEFAULT_HIGHLIGHTER_CURSOR_SIZE},
    icon_names,
};

fn sync_arrow_option_selection(list: &GtkBox, selected_index: usize) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        if index == selected_index {
            button.add_css_class("editor-arrow-inspector-option-active");
        } else {
            button.remove_css_class("editor-arrow-inspector-option-active");
        }

        if let Some(content) = button.child() {
            if let Ok(row) = content.downcast::<GtkBox>() {
                if let Some(check_icon) = row.last_child() {
                    if let Ok(widget) = check_icon.downcast::<gtk4::Widget>() {
                        widget.set_visible(index == selected_index);
                    }
                }
            }
        }

        index += 1;
    }
}

fn sync_number_option_selection(list: &GtkBox, selected_index: usize, active_class: &str) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        let is_active = index == selected_index;
        if is_active {
            button.add_css_class(active_class);
        } else {
            button.remove_css_class(active_class);
        }

        if let Some(content) = button.child() {
            if let Ok(row) = content.downcast::<GtkBox>() {
                if let Some(check_icon) = row.last_child() {
                    if let Ok(widget) = check_icon.downcast::<gtk4::Widget>() {
                        widget.set_visible(is_active);
                    }
                }
            }
        }

        index += 1;
    }
}

pub(super) struct ToolOptionsParts<'a> {
    pub pen_weight_button: &'a Button,
    pub pen_weight_list: &'a GtkBox,
    pub highlighter_weight_list: &'a GtkBox,
    pub obfuscate_method_button: &'a Button,
    pub obfuscate_method_list: &'a GtkBox,
    pub arrow_style_button: &'a Button,
    pub arrow_style_list: &'a GtkBox,
    pub arrow_thickness_list: &'a GtkBox,
    pub stroke_size_button: &'a Button,
    pub stroke_size_list: &'a GtkBox,
    pub inverse_direction_toggle: &'a CheckButton,
    pub number_options_list: &'a GtkBox,
    pub number_start_entry: &'a Entry,
    pub number_inc_btn: &'a Button,
    pub number_dec_btn: &'a Button,
    pub number_size_button: &'a Button,
    pub number_size_list: &'a GtkBox,
    pub color_buttons: &'a [Button],
    pub color_picker_dot: &'a GtkBox,
    pub color_class_names: &'a [&'static str],
    pub set_picker_panel_visibility: &'a Rc<dyn Fn(bool)>,
    pub size_slider: &'a Scale,
}

/// Wire inspector/toolbar option callbacks (weight, style, numbering, palette, size).
pub(super) fn wire_tool_options(
    parts: ToolOptionsParts<'_>,
    window: &ApplicationWindow,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    sync_picker_from_color: &Rc<dyn Fn(DrawColor)>,
    sync_size_control: &Rc<dyn Fn()>,
    rebuild_effects_async: &Rc<dyn Fn()>,
    apply_picker_color: &Rc<dyn Fn(DrawColor)>,
) {
    let ToolOptionsParts {
        pen_weight_button,
        pen_weight_list,
        highlighter_weight_list,
        obfuscate_method_button,
        obfuscate_method_list,
        arrow_style_button,
        arrow_style_list,
        arrow_thickness_list,
        stroke_size_button,
        stroke_size_list,
        inverse_direction_toggle,
        number_options_list,
        number_start_entry,
        number_inc_btn,
        number_dec_btn,
        number_size_button,
        number_size_list,
        color_buttons,
        color_picker_dot,
        color_class_names,
        set_picker_panel_visibility,
        size_slider,
    } = parts;

    // The first Highlighter row restores text-aware sizing. Thickness rows below
    // it explicitly select Freehand mode.
    if let Some(child) = highlighter_weight_list.first_child() {
        if let Ok(button) = child.downcast::<Button>() {
            let state_detect_text = state.clone();
            let drawing_area_detect_text = drawing_area.downgrade();
            let highlighter_list_detect_text = highlighter_weight_list.clone();
            let window_detect_text = window.clone();
            button.connect_clicked(move |_| {
                let color = {
                    let mut st = state_detect_text.lock().unwrap();
                    st.set_highlighter_mode(HighlighterMode::TextAware);
                    (
                        st.selected_color.r,
                        st.selected_color.g,
                        st.selected_color.b,
                        0.4,
                    )
                };
                sync_arrow_option_selection(&highlighter_list_detect_text, 0);
                set_highlighter_cursor(&window_detect_text, DEFAULT_HIGHLIGHTER_CURSOR_SIZE, color);
                if let Some(area) = drawing_area_detect_text.upgrade() {
                    area.queue_draw();
                }
            });
        }
    }

    // Wire up pen weight list items for pen and highlighter freehand mode.
    // NOTE: Do not remove children here; that would empty the popover and nothing would display.
    let weights = [
        PenWeight::Small,
        PenWeight::Medium,
        PenWeight::Large,
        PenWeight::ExtraLarge,
    ];

    let pen_weight_button_for_closure = pen_weight_button.clone();
    let drawing_area_for_weight = drawing_area.downgrade();
    let window_pen_weight = window.clone();

    for (weight_list, highlighter_offset) in [
        (pen_weight_list.clone(), 0usize),
        (highlighter_weight_list.clone(), 1usize),
    ] {
        let mut weight_idx = 0usize;
        let mut child_opt = weight_list.first_child();
        if highlighter_offset > 0 {
            child_opt = child_opt.and_then(|child| child.next_sibling());
        }
        while let Some(child) = child_opt {
            // Grab next sibling before we do anything else
            child_opt = child.next_sibling();

            let Ok(button) = child.clone().downcast::<Button>() else {
                continue;
            };

            let Some(&weight) = weights.get(weight_idx) else {
                break;
            };
            weight_idx += 1;

            let selected_index = highlighter_offset + weight_idx - 1;
            let state_for_weight = state.clone();
            let drawing_area_weight = drawing_area_for_weight.clone();
            let pen_weight_button_clone = pen_weight_button_for_closure.clone();
            let window_for_weight = window_pen_weight.clone();
            let weight_list_for_sync = weight_list.clone();

            button.connect_clicked(move |b| {
                {
                    let mut st = state_for_weight.lock().unwrap();
                    let is_highlighter = st.selected_tool == Tool::Highlighter;
                    let is_pen = st.selected_tool == Tool::Pen;
                    if is_highlighter {
                        // Same path as the floating highlighter bar: leave text-aware
                        // sizing, apply the preset to the selected stroke, and keep it
                        // as the brush for the next stroke.
                        st.set_highlighter_weight(weight);
                    } else {
                        // Same path as the floating pen bar: remember the brush weight
                        // and resize the selected stroke with it.
                        st.set_pen_weight_and_apply(weight);
                    }
                    drop(st);

                    if is_pen {
                        let st = state_for_weight.lock().unwrap();
                        update_pen_cursor(&window_for_weight, &st);
                    } else if is_highlighter {
                        let st = state_for_weight.lock().unwrap();
                        set_highlighter_cursor(
                            &window_for_weight,
                            weight.highlighter_stroke_width(),
                            (
                                st.selected_color.r,
                                st.selected_color.g,
                                st.selected_color.b,
                                0.4,
                            ),
                        );
                    }
                }

                let icon = gtk4::Image::from_icon_name(weight.icon_name());
                icon.set_pixel_size(weight.icon_pixel_size());
                pen_weight_button_clone.set_child(Some(&icon));
                sync_arrow_option_selection(&weight_list_for_sync, selected_index);

                // Close the popover
                if let Some(popover) = b.ancestor(Popover::static_type()) {
                    popover.downcast::<Popover>().unwrap().popdown();
                }

                if let Some(area) = drawing_area_weight.upgrade() {
                    area.queue_draw();
                }
            });
        }
    }

    // Wire up obfuscate method list items
    // NOTE: Do not remove children here; that would empty the popover and nothing would display.
    let methods = [
        ObfuscateMethod::Pixelate,
        ObfuscateMethod::Blur,
        ObfuscateMethod::Blackout,
    ];

    let obfuscate_method_button = obfuscate_method_button.clone();
    let rebuild_effects_async_obfuscate_method = rebuild_effects_async.clone();
    let sync_size_control_obfuscate_method = sync_size_control.clone();

    let mut method_idx = 0usize;
    let mut child_opt = obfuscate_method_list.first_child();
    while let Some(child) = child_opt {
        // Grab next sibling before we do anything else
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        let Some(&method) = methods.get(method_idx) else {
            break;
        };
        method_idx += 1;

        let state_obfuscate_method = state.clone();
        let drawing_area_obfuscate_method = drawing_area.downgrade();
        let obfuscate_method_button = obfuscate_method_button.clone();
        let rebuild_effects_async_obfuscate_method = rebuild_effects_async_obfuscate_method.clone();
        let sync_size_control_obfuscate_method = sync_size_control_obfuscate_method.clone();

        button.connect_clicked(move |b| {
            {
                let mut st = state_obfuscate_method.lock().unwrap();
                st.set_obfuscate_method(method);
            }

            // Update the method button icon to reflect current selection.
            if let Some(child) = obfuscate_method_button.child() {
                if let Ok(img) = child.downcast::<Image>() {
                    let icon_name = match method {
                        ObfuscateMethod::Pixelate => icon_names::VIEW_GRID,
                        ObfuscateMethod::Blur => icon_names::BLUR,
                        ObfuscateMethod::Blackout => icon_names::MEDIA_PLAYBACK_STOP,
                    };
                    img.set_icon_name(Some(icon_name));
                }
            }

            // Rebuild effects so existing obfuscate annotations update immediately.
            rebuild_effects_async_obfuscate_method();

            // Sync toolbar sizing / slider state.
            sync_size_control_obfuscate_method();

            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }
            if let Some(area) = drawing_area_obfuscate_method.upgrade() {
                area.queue_draw();
            }
        });
    }

    // Wire up arrow style list items
    let styles = ArrowStyle::ALL;

    let arrow_style_button = arrow_style_button.clone();
    let arrow_style_list_for_sync = arrow_style_list.clone();

    let mut style_idx = 0usize;
    let mut child_opt = arrow_style_list.first_child();
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        let Some(&style) = styles.get(style_idx) else {
            break;
        };
        let selected_index = style_idx;
        style_idx += 1;

        let state_arrow_style = state.clone();
        let drawing_area_arrow_style = drawing_area.downgrade();
        let arrow_style_button = arrow_style_button.clone();
        let arrow_style_list = arrow_style_list_for_sync.clone();

        button.connect_clicked(move |b| {
            {
                let mut st = state_arrow_style.lock().unwrap();
                st.set_arrow_style(style);
                let _ = st.set_selected_arrow_style(style);
            }

            let icon = arrow_style_toolbar_icon(style);
            set_button_tool_icon(&arrow_style_button, icon.clone(), toolbar_icon_size(&icon));
            sync_arrow_option_selection(&arrow_style_list, selected_index);

            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }
            if let Some(area) = drawing_area_arrow_style.upgrade() {
                area.queue_draw();
            }
        });
    }

    // Wire up stroke size list items for arrow/line tools
    let stroke_sizes: [(f64, PenWeight); 4] = [
        (2.0, PenWeight::Small),
        (4.0, PenWeight::Medium),
        (7.0, PenWeight::Large),
        (12.0, PenWeight::ExtraLarge),
    ];

    let arrow_thickness_list_for_sync = arrow_thickness_list.clone();

    let stroke_size_button_for_closure = stroke_size_button.clone();
    let drawing_area_for_stroke = drawing_area.downgrade();

    let mut stroke_idx = 0usize;
    let mut child_opt = stroke_size_list.first_child();
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        let Some(&(size, weight)) = stroke_sizes.get(stroke_idx) else {
            break;
        };
        stroke_idx += 1;

        let selected_index = stroke_idx - 1;
        let state_stroke = state.clone();
        let drawing_area_stroke = drawing_area_for_stroke.clone();
        let stroke_size_button_clone = stroke_size_button_for_closure.clone();
        let stroke_size_list_for_sync = stroke_size_list.clone();

        button.connect_clicked(move |b| {
            {
                let mut st = state_stroke.lock().unwrap();
                st.set_stroke_size(size);
            }

            // Update the trigger button icon to reflect selected size
            let icon = gtk4::Image::from_icon_name(weight.icon_name());
            icon.set_pixel_size(weight.icon_pixel_size());
            stroke_size_button_clone.set_child(Some(&icon));
            sync_arrow_option_selection(&stroke_size_list_for_sync, selected_index);

            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }
            if let Some(area) = drawing_area_stroke.upgrade() {
                area.queue_draw();
            }
        });
    }

    let arrow_thickness_sizes: [(f64, PenWeight); 4] = [
        (2.0, PenWeight::Small),
        (4.0, PenWeight::Medium),
        (7.0, PenWeight::Large),
        (12.0, PenWeight::ExtraLarge),
    ];

    let mut thickness_idx = 0usize;
    let mut child_opt = arrow_thickness_list.first_child();
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        let Some(&(size, weight)) = arrow_thickness_sizes.get(thickness_idx) else {
            break;
        };
        let selected_index = thickness_idx;
        thickness_idx += 1;

        let state_stroke = state.clone();
        let drawing_area_stroke = drawing_area.downgrade();
        let stroke_size_button_clone = stroke_size_button.clone();
        let arrow_thickness_list = arrow_thickness_list_for_sync.clone();

        button.connect_clicked(move |_| {
            {
                let mut st = state_stroke.lock().unwrap();
                st.set_stroke_size(size);
            }

            let icon = gtk4::Image::from_icon_name(weight.icon_name());
            icon.set_pixel_size(weight.icon_pixel_size());
            stroke_size_button_clone.set_child(Some(&icon));
            sync_arrow_option_selection(&arrow_thickness_list, selected_index);

            if let Some(area) = drawing_area_stroke.upgrade() {
                area.queue_draw();
            }
        });
    }

    inverse_direction_toggle.connect_toggled({
        let state = state.clone();
        let drawing_area = drawing_area.downgrade();
        move |toggle| {
            {
                let mut st = state.lock().unwrap();
                let next = toggle.is_active();
                if st.inverse_arrow_direction != next {
                    st.inverse_arrow_direction = next;
                    let _ = st.reverse_selected_arrow_action();
                }
            }

            if let Some(area) = drawing_area.upgrade() {
                area.queue_draw();
            }
        }
    });

    let refresh_number_start_display: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let number_start_entry = number_start_entry.clone();
        move || {
            let st = state.lock().unwrap();
            number_start_entry.set_text(&st.active_number_start_display());
        }
    });

    // Wire up number style options
    let styles = NumberingStyle::ALL;
    let state_number_style = state.clone();
    let drawing_area_number_style = drawing_area.downgrade();
    let refresh_number_start_display_style = refresh_number_start_display.clone();

    let mut style_idx = 0usize;
    let mut child_opt = number_options_list.first_child();
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        if !button
            .css_classes()
            .iter()
            .any(|c| c == "editor-number-style-option")
        {
            continue;
        }

        let Some(&style) = styles.get(style_idx) else {
            break;
        };
        style_idx += 1;

        let state_style = state_number_style.clone();
        let drawing_area_style = drawing_area_number_style.clone();
        let refresh_display = refresh_number_start_display_style.clone();
        let number_options_list_sync = number_options_list.clone();

        button.connect_clicked(move |_| {
            state_style.lock().unwrap().set_numbering_style(style);
            sync_number_option_selection(
                &number_options_list_sync,
                style_idx - 1,
                "editor-number-style-option-active",
            );
            refresh_display();

            if let Some(area) = drawing_area_style.upgrade() {
                area.queue_draw();
            }
        });
    }

    // Wire up start +/- controls
    let refresh_number_start_display_inc = refresh_number_start_display.clone();
    number_inc_btn.connect_clicked({
        let state = state.clone();
        move |_| {
            {
                let mut st = state.lock().unwrap();
                let next = st.active_number_start().saturating_add(1);
                st.set_active_number_start(next);
            }
            refresh_number_start_display_inc();
        }
    });

    let refresh_number_start_display_dec = refresh_number_start_display.clone();
    number_dec_btn.connect_clicked({
        let state = state.clone();
        move |_| {
            {
                let mut st = state.lock().unwrap();
                let current = st.active_number_start();
                if current > 1 {
                    st.set_active_number_start(current - 1);
                }
            }
            refresh_number_start_display_dec();
        }
    });

    refresh_number_start_display();

    // Wire up number size options
    let sizes = NumberSize::ALL;

    let state_number_size = state.clone();
    let drawing_area_number_size = drawing_area.downgrade();

    let mut size_idx = 0usize;
    let mut child_opt = number_size_list.first_child();
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };

        let Some(&size) = sizes.get(size_idx) else {
            break;
        };
        size_idx += 1;

        let state_size = state_number_size.clone();
        let drawing_area_size = drawing_area_number_size.clone();
        let number_size_btn = number_size_button.clone();
        let number_size_list_sync = number_size_list.clone();

        button.connect_clicked(move |b| {
            state_size.lock().unwrap().set_number_size(size);

            sync_number_option_selection(
                &number_size_list_sync,
                size_idx - 1,
                "editor-number-size-option-active",
            );

            // Close the size popover
            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }

            // Also close the main number options popover
            if let Some(parent) = number_size_btn.parent() {
                if let Some(popover) = parent.ancestor(Popover::static_type()) {
                    popover.downcast::<Popover>().unwrap().popdown();
                }
            }

            if let Some(area) = drawing_area_size.upgrade() {
                area.queue_draw();
            }
        });
    }

    for (index, button) in color_buttons.iter().enumerate() {
        let state_color = state.clone();
        let apply_picker_color_sw = apply_picker_color.clone();
        let drawing_area_color = drawing_area.downgrade();
        let color_buttons_group = color_buttons.to_vec();
        let color_picker_dot_group = color_picker_dot.clone();
        let color_class_names_group = color_class_names.to_vec();
        let set_picker_visible_group = set_picker_panel_visibility.clone();
        let sync_picker_from_color_group = sync_picker_from_color.clone();
        button.connect_clicked(move |_| {
            // A swatch goes to the colour the shared picker is pointed at: the
            // drawing colour from the toolbar chip, the background fill while
            // the Colors panel is editing it. The Background fill belongs to
            // the shared Motion runtime, so that write has to travel through
            // the picker's own background setter — a direct EditorState write
            // would be reverted by the tick sync.
            let has_active_text = state_color.lock().unwrap().active_text_input.is_some();
            apply_picker_color_sw(DRAW_COLORS[index]);
            sync_picker_from_color_group(DRAW_COLORS[index]);

            color_picker::set_active_color_picker_state(
                &color_buttons_group,
                &color_picker_dot_group,
                &color_class_names_group,
                index,
            );
            set_picker_visible_group(false);
            if let Some(area) = drawing_area_color.upgrade() {
                if has_active_text {
                    area.grab_focus();
                }
                area.queue_draw();
            }
        });
    }

    let state_size = state.clone();
    let drawing_area_size = drawing_area.downgrade();
    let rebuild_effects_async_size = rebuild_effects_async.clone();
    size_slider.connect_value_changed(move |slider| {
        let value = slider.value();
        if state_size
            .lock()
            .unwrap()
            .set_active_size_without_rebuild(value)
        {
            rebuild_effects_async_size();
            if let Some(area) = drawing_area_size.upgrade() {
                area.queue_draw();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_options_cover_weight_style_numbering_palette_and_size() {
        let source = include_str!("options.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("st.set_pen_weight_and_apply(weight)")
                && production.contains("st.set_highlighter_mode(HighlighterMode::TextAware)")
                && production.contains("st.set_obfuscate_method(method)")
                && production.contains("rebuild_effects_async_obfuscate_method()")
                && production.contains("st.set_arrow_style(style)")
                && production.contains("st.set_stroke_size(size)")
                && production.contains("st.inverse_arrow_direction = next")
                && production.contains("st.reverse_selected_arrow_action()")
                && production.contains("set_numbering_style(style)")
                && production.contains("set_number_size(size)")
                && !production.contains("set_crop_background_color")
                && production.contains("apply_picker_color_sw(DRAW_COLORS[index]);")
                && production.contains("set_active_size_without_rebuild(value)"),
            "tool options must retain weight, style, direction, numbering, palette, size, and obfuscate policies"
        );
    }

    /// The popover's palette writes through the shared picker, so one swatch
    /// reaches the drawing colour from the toolbar chip and the background fill
    /// — which the shared Motion runtime owns — from the Colors panel. Keeping
    /// a second copy of that routing here is what let the palette and the
    /// picker's own canvas disagree about which colour they were editing.
    #[test]
    fn tool_options_palette_defers_to_the_shared_picker_target() {
        let source = include_str!("options.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("apply_picker_color_sw(DRAW_COLORS[index]);")
                && !production.contains("set_background_fill_sw")
                && !production.contains("st.set_color_index(index)"),
            "the palette must not re-implement the picker's background routing"
        );
    }
}
