use gtk4::{
    gdk, glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    DrawingArea, EventControllerFocus, EventControllerKey, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, GestureClick, GestureDrag, Image, Label,
    Overlay, Popover, Scale, ScrolledWindow,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::{
    color::{palette_index_for_color, DRAG_REDRAW_INTERVAL_US, DRAW_COLORS},
    io_ops::{copy_uri_to_clipboard, save_edited_image},
    numbering_style::{NumberSize, NumberingStyle},
    render::cursor_position_for_text_point,
    state::EditorState,
    types::{
        tool_button_index, tool_shortcut_target, ArrowStyle, BackgroundStyle, DrawColor,
        FontSettings, FontStyle, MoveHandle, ObfuscateMethod, Point, TextAlignment, TextDecoration,
        Tool, ViewTransform,
    },
    ui_support::{
        arrow_style_toolbar_icon, set_active_tool_button, set_button_tool_icon,
        set_crop_apply_button_state, toolbar_icon_size,
    },
};
use crate::annotations::save_annotations;

const MOVE_HANDLE_DRAG_RADIUS: f64 = 10.0;
const RESIZE_HANDLE_DRAG_SIZE: f64 = 18.0;
const ARROW_CLICK_NOOP_DISTANCE: f64 = 3.0;
const TEXT_SIZE_OPTIONS: [i32; 12] = [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72];
const TEXT_FONT_FAMILIES: [&str; 5] = ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"];
const MIN_ZOOM_LEVEL: f64 = 0.25;
const MAX_ZOOM_LEVEL: f64 = 6.0;
const ZOOM_STEP: f64 = 1.1;
use super::super::pen_weight::{HighlighterMode, PenWeight};
use super::{
    canvas::{
        eyedropper_loupe_position, sample_editor_color_at_point, sample_rendered_color_at_point,
    },
    color_picker,
    cursor::{cursor_name_for_view_point, set_window_cursor_name},
    icon_names,
};

fn clamp_zoom_level(value: f64) -> f64 {
    value.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL)
}

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

fn sync_text_option_selection(list: &GtkBox, selected_index: Option<usize>) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        let is_active = selected_index == Some(index);
        if is_active {
            button.add_css_class("editor-text-inspector-option-active");
        } else {
            button.remove_css_class("editor-text-inspector-option-active");
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

pub(super) struct EventContext {
    pub app: Application,
    pub window: ApplicationWindow,
    pub path: PathBuf,
    pub state: Arc<Mutex<EditorState>>,
    pub transform: Arc<Mutex<ViewTransform>>,
    pub drawing_area: DrawingArea,
    pub tool_buttons: Vec<Button>,
    pub select_btn: Button,
    pub crop_btn: Button,
    pub background_btn: Button,
    pub draw_btn: Button,
    pub arrow_btn: Button,
    pub line_btn: Button,
    pub box_btn: Button,
    pub circle_btn: Button,
    pub text_btn: Button,
    pub number_btn: Button,
    pub highlighter_btn: Button,
    pub obfuscate_btn: Button,
    pub focus_btn: Button,
    pub traffic_close: Button,
    pub traffic_minimize: Button,
    pub traffic_zoom: Button,
    pub canvas_overlay: Overlay,
    pub canvas_scroller: ScrolledWindow,
    pub zoom_button: Button,
    pub zoom_label: Label,
    pub zoom_header_label: Label,
    pub zoom_popup: GtkBox,
    pub zoom_minus_btn: Button,
    pub zoom_plus_btn: Button,
    pub zoom_in_btn: Button,
    pub zoom_out_btn: Button,
    pub fit_to_screen_btn: Button,
    pub zoom_to_selection_btn: Button,
    pub zoom_level: Rc<Cell<f64>>,
    pub copy_btn: Button,
    #[allow(dead_code)]
    pub upload_btn: Button,
    pub color_buttons: Vec<Button>,
    pub color_picker_dot: GtkBox,
    pub color_class_names: Vec<&'static str>,
    pub color_popover: Popover,
    pub size_slider: Scale,
    pub text_size_label: Label,
    pub font_family_label: Label,
    pub text_size_list: gtk4::Box,
    pub font_family_list: gtk4::Box,
    pub apply_crop_btn: Button,
    pub crop_reset_btn: Button,

    pub undo_btn: Button,
    pub redo_btn: Button,
    pub delete_selected_btn: Button,
    pub save_btn: Button,
    pub eyedropper_mode: Rc<Cell<bool>>,
    pub eyedropper_from_sidebar: Rc<Cell<bool>>,
    pub eyedropper_point: Rc<RefCell<Option<Point>>>,
    pub eyedropper_rendered: Rc<RefCell<Option<RgbaImage>>>,
    pub canvas_eyedropper_ring: DrawingArea,
    pub update_toolbar_for_tool: Rc<dyn Fn(Tool)>,
    pub update_crop_size_fields: Rc<dyn Fn()>,
    pub update_canvas_content_size: Rc<dyn Fn()>,
    pub sync_picker_for_active_tool: Rc<dyn Fn()>,
    pub sync_picker_from_color: Rc<dyn Fn(DrawColor)>,
    pub apply_picker_color_to_editor: Rc<dyn Fn(DrawColor)>,
    pub add_color_to_custom_slots: Rc<dyn Fn(DrawColor)>,
    pub set_picker_panel_visibility: Rc<dyn Fn(bool)>,
    pub sync_select_inspector: Rc<dyn Fn()>,
    pub sync_size_control: Rc<dyn Fn()>,
    pub rebuild_effects_async: Rc<dyn Fn()>,
    pub obfuscate_method_button: Button,
    pub obfuscate_method_list: gtk4::Box,
    pub pen_weight_button: Button,
    pub pen_weight_list: gtk4::Box,
    pub highlighter_weight_list: gtk4::Box,
    pub number_options_list: gtk4::Box,
    pub number_start_entry: gtk4::Entry,
    pub number_inc_btn: Button,
    pub number_dec_btn: Button,
    pub number_size_button: Button,
    pub number_size_list: gtk4::Box,
    pub arrow_style_button: Button,
    pub arrow_style_list: gtk4::Box,
    pub arrow_thickness_list: gtk4::Box,
    pub inverse_direction_toggle: CheckButton,
    pub stroke_size_button: Button,
    pub stroke_size_list: gtk4::Box,
}

pub(super) fn wire_editor_events(ctx: EventContext) {
    let EventContext {
        app,
        window,
        path,
        state,
        transform,
        drawing_area,
        tool_buttons,
        select_btn,
        crop_btn,
        background_btn,
        draw_btn,
        arrow_btn,
        line_btn,
        box_btn,
        circle_btn,
        text_btn,
        number_btn,
        highlighter_btn,
        obfuscate_btn,
        focus_btn,
        traffic_close,
        traffic_minimize,
        traffic_zoom,
        canvas_overlay: _canvas_overlay,
        canvas_scroller,
        zoom_button,
        zoom_label: _zoom_label,
        zoom_header_label,
        zoom_popup,
        zoom_minus_btn,
        zoom_plus_btn,
        zoom_in_btn,
        zoom_out_btn,
        fit_to_screen_btn,
        zoom_to_selection_btn,
        zoom_level,
        copy_btn,
        upload_btn,
        color_buttons,
        color_picker_dot,
        color_class_names,
        color_popover,
        size_slider,
        text_size_label,
        font_family_label,
        text_size_list,
        font_family_list,
        apply_crop_btn,
        crop_reset_btn,
        undo_btn,
        redo_btn,
        delete_selected_btn,
        save_btn,
        eyedropper_mode,
        eyedropper_from_sidebar,
        eyedropper_point,
        eyedropper_rendered,
        canvas_eyedropper_ring,
        update_toolbar_for_tool,
        update_crop_size_fields,
        update_canvas_content_size,
        sync_picker_for_active_tool,
        sync_picker_from_color,
        apply_picker_color_to_editor,
        add_color_to_custom_slots,
        set_picker_panel_visibility,
        sync_select_inspector,
        sync_size_control,
        rebuild_effects_async,
        obfuscate_method_button,
        obfuscate_method_list,
        pen_weight_button,
        pen_weight_list,
        highlighter_weight_list,
        number_options_list,
        number_start_entry,
        number_inc_btn,
        number_dec_btn,
        number_size_button,
        number_size_list,
        arrow_style_button,
        arrow_style_list,
        arrow_thickness_list,
        inverse_direction_toggle,
        stroke_size_button,
        stroke_size_list,
    } = ctx;

    let drag_start_transform = Rc::new(RefCell::new(None::<ViewTransform>));

    let state_select = state.clone();
    let drawing_area_select = drawing_area.downgrade();
    let buttons_select = tool_buttons.clone();
    let apply_crop_btn_select = apply_crop_btn.clone();
    let update_toolbar_for_tool_select = update_toolbar_for_tool.clone();
    let sync_size_control_select = sync_size_control.clone();
    let sync_select_inspector_select = sync_select_inspector.clone();
    let rebuild_effects_async_select = rebuild_effects_async.clone();
    select_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_select, tool_button_index(Tool::Select));
        if state_select
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Select)
        {
            rebuild_effects_async_select();
        }
        update_toolbar_for_tool_select(Tool::Select);
        sync_select_inspector_select();
        sync_size_control_select();
        set_crop_apply_button_state(&apply_crop_btn_select, false, false);
        if let Some(area) = drawing_area_select.upgrade() {
            area.queue_draw();
        }
    });

    let window_minimize = window.downgrade();
    traffic_minimize.connect_clicked(move |_| {
        if let Some(window) = window_minimize.upgrade() {
            window.minimize();
        }
    });

    let zoomed_state = Rc::new(Cell::new(false));
    let zoomed_state_btn = zoomed_state.clone();
    let window_zoom = window.downgrade();
    traffic_zoom.connect_clicked(move |_| {
        if let Some(window) = window_zoom.upgrade() {
            let next_zoomed = !zoomed_state_btn.get();
            zoomed_state_btn.set(next_zoomed);
            window.set_fullscreened(next_zoomed);
        }
    });

    let state_crop = state.clone();
    let drawing_area_crop = drawing_area.downgrade();
    let buttons_crop = tool_buttons.clone();
    let apply_crop_btn_crop = apply_crop_btn.clone();
    let update_toolbar_for_tool_crop = update_toolbar_for_tool.clone();
    let update_crop_size_fields_crop = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_crop = sync_picker_for_active_tool.clone();
    let sync_size_control_crop = sync_size_control.clone();
    let rebuild_effects_async_crop = rebuild_effects_async.clone();
    crop_btn.connect_clicked(move |_| {
        let (next_tool, has_selection) = {
            let mut st = state_crop.lock().unwrap();
            let rebuild = if st.selected_tool == Tool::Crop {
                let r = st.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, false, r)
            } else {
                let r = st.set_tool_without_rebuild(Tool::Crop);
                st.ensure_crop_selection_initialized();
                (Tool::Crop, st.crop_selection.is_some(), r)
            };
            if rebuild.2 {
                rebuild_effects_async_crop();
            }
            (rebuild.0, rebuild.1)
        };

        set_active_tool_button(&buttons_crop, tool_button_index(next_tool));
        update_toolbar_for_tool_crop(next_tool);
        sync_picker_for_active_tool_crop();
        sync_size_control_crop();
        set_crop_apply_button_state(
            &apply_crop_btn_crop,
            matches!(next_tool, Tool::Crop),
            has_selection,
        );
        update_crop_size_fields_crop();
        if let Some(area) = drawing_area_crop.upgrade() {
            area.queue_draw();
        }
    });

    let state_background = state.clone();
    let drawing_area_background = drawing_area.downgrade();
    let buttons_background = tool_buttons.clone();
    let apply_crop_btn_background = apply_crop_btn.clone();
    let update_toolbar_for_tool_background = update_toolbar_for_tool.clone();
    let sync_picker_for_active_tool_background = sync_picker_for_active_tool.clone();
    let sync_size_control_background = sync_size_control.clone();
    let rebuild_effects_async_background = rebuild_effects_async.clone();
    background_btn.connect_clicked(move |_| {
        let next_tool = {
            let mut st = state_background.lock().unwrap();
            let rebuild = if st.selected_tool == Tool::Background {
                let r = st.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, r)
            } else {
                let r = st.set_tool_without_rebuild(Tool::Background);
                (Tool::Background, r)
            };
            if rebuild.1 {
                rebuild_effects_async_background();
            }
            rebuild.0
        };

        set_active_tool_button(&buttons_background, tool_button_index(next_tool));

        update_toolbar_for_tool_background(next_tool);
        sync_picker_for_active_tool_background();
        sync_size_control_background();
        set_crop_apply_button_state(&apply_crop_btn_background, false, false);
        if let Some(area) = drawing_area_background.upgrade() {
            area.queue_draw();
        }
    });

    let state_draw_mode = state.clone();
    let drawing_area_draw_mode = drawing_area.downgrade();
    let buttons_draw_mode = tool_buttons.clone();
    let apply_crop_btn_draw_mode = apply_crop_btn.clone();
    let update_toolbar_for_tool_draw_mode = update_toolbar_for_tool.clone();
    let sync_size_control_draw = sync_size_control.clone();
    let rebuild_effects_async_draw = rebuild_effects_async.clone();
    let window_draw = window.clone();
    draw_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_draw_mode, tool_button_index(Tool::Pen));
        if state_draw_mode
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Pen)
        {
            rebuild_effects_async_draw();
        }
        update_toolbar_for_tool_draw_mode(Tool::Pen);
        sync_size_control_draw();
        set_crop_apply_button_state(&apply_crop_btn_draw_mode, false, false);
        {
            let st = state_draw_mode.lock().unwrap();
            super::cursor::update_pen_cursor(&window_draw, &st);
        }
        if let Some(area) = drawing_area_draw_mode.upgrade() {
            area.queue_draw();
        }
    });

    let state_arrow = state.clone();
    let drawing_area_arrow = drawing_area.downgrade();
    let buttons_arrow = tool_buttons.clone();
    let apply_crop_btn_arrow = apply_crop_btn.clone();
    let update_toolbar_for_tool_arrow = update_toolbar_for_tool.clone();
    let sync_size_control_arrow = sync_size_control.clone();
    let rebuild_effects_async_arrow = rebuild_effects_async.clone();
    arrow_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_arrow, tool_button_index(Tool::Arrow));
        if state_arrow
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Arrow)
        {
            rebuild_effects_async_arrow();
        }
        update_toolbar_for_tool_arrow(Tool::Arrow);
        sync_size_control_arrow();
        set_crop_apply_button_state(&apply_crop_btn_arrow, false, false);
        if let Some(area) = drawing_area_arrow.upgrade() {
            area.queue_draw();
        }
    });

    let state_line = state.clone();
    let drawing_area_line = drawing_area.downgrade();
    let buttons_line = tool_buttons.clone();
    let apply_crop_btn_line = apply_crop_btn.clone();
    let update_toolbar_for_tool_line = update_toolbar_for_tool.clone();
    let sync_size_control_line = sync_size_control.clone();
    let rebuild_effects_async_line = rebuild_effects_async.clone();
    line_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_line, tool_button_index(Tool::Line));
        if state_line
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Line)
        {
            rebuild_effects_async_line();
        }
        update_toolbar_for_tool_line(Tool::Line);
        sync_size_control_line();
        set_crop_apply_button_state(&apply_crop_btn_line, false, false);
        if let Some(area) = drawing_area_line.upgrade() {
            area.queue_draw();
        }
    });

    let apply_zoom_change: Rc<dyn Fn(f64)> = Rc::new({
        let zoom_level = zoom_level.clone();
        let update_canvas_content_size = update_canvas_content_size.clone();
        let drawing_area = drawing_area.clone();
        move |next_zoom| {
            zoom_level.set(clamp_zoom_level(next_zoom));
            update_canvas_content_size();
            drawing_area.queue_draw();
        }
    });

    // Make popup focusable and close on focus loss (click outside)
    zoom_popup.set_can_focus(true);
    let zoom_popup_focus = zoom_popup.clone();
    let focus_controller = EventControllerFocus::new();
    focus_controller.connect_leave(move |_| {
        zoom_popup_focus.set_visible(false);
    });
    zoom_popup.add_controller(focus_controller);

    let zoom_popup_btn = zoom_popup.clone();
    zoom_button.connect_clicked(move |_| {
        let becoming_visible = !zoom_popup_btn.is_visible();
        zoom_popup_btn.set_visible(becoming_visible);
        if becoming_visible {
            zoom_popup_btn.grab_focus();
        }
    });

    // Close popup when clicking outside of it (on the window)
    let zoom_popup_window_click = zoom_popup.clone();
    let zoom_button_for_click = zoom_button.clone();
    let window_for_click = window.clone();
    let window_click = GestureClick::new();
    window_click.set_button(0); // Listen for all buttons
    window_click.connect_pressed(move |_, _, click_x, click_y| {
        if !zoom_popup_window_click.is_visible() {
            return;
        }

        // Get popup position relative to window
        let (popup_win_x, popup_win_y) = zoom_popup_window_click
            .translate_coordinates(&window_for_click, 0.0, 0.0)
            .unwrap_or((0.0, 0.0));
        let popup_alloc = zoom_popup_window_click.allocation();

        let in_popup = click_x >= popup_win_x
            && click_x <= popup_win_x + popup_alloc.width() as f64
            && click_y >= popup_win_y
            && click_y <= popup_win_y + popup_alloc.height() as f64;

        // Check if click is on the zoom button (toggle button)
        let (btn_x, btn_y) = zoom_button_for_click
            .translate_coordinates(&window_for_click, 0.0, 0.0)
            .unwrap_or((0.0, 0.0));
        let btn_alloc = zoom_button_for_click.allocation();
        let in_button = click_x >= btn_x
            && click_x <= btn_x + btn_alloc.width() as f64
            && click_y >= btn_y
            && click_y <= btn_y + btn_alloc.height() as f64;

        if !in_popup && !in_button {
            zoom_popup_window_click.set_visible(false);
        }
    });
    window.add_controller(window_click);

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_in = zoom_level.clone();
    let zoom_popup_in = zoom_popup.clone();
    zoom_in_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(zoom_level_in.get() * ZOOM_STEP);
        let _ = b;
        zoom_popup_in.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_out = zoom_level.clone();
    let zoom_popup_out = zoom_popup.clone();
    zoom_out_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(zoom_level_out.get() / ZOOM_STEP);
        let _ = b;
        zoom_popup_out.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_popup_fit = zoom_popup.clone();
    fit_to_screen_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(1.0);
        let _ = b;
        zoom_popup_fit.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_minus = zoom_level.clone();
    zoom_minus_btn.connect_clicked(move |_| {
        apply_zoom_change_btn(zoom_level_minus.get() / ZOOM_STEP);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_plus = zoom_level.clone();
    zoom_plus_btn.connect_clicked(move |_| {
        apply_zoom_change_btn(zoom_level_plus.get() * ZOOM_STEP);
    });

    // Make the header label clickable to reset zoom to 100%
    let apply_zoom_change_label = apply_zoom_change.clone();
    let label_click = GestureClick::new();
    label_click.connect_pressed(move |_, _, _, _| {
        apply_zoom_change_label(1.0);
    });
    zoom_header_label.add_controller(label_click);

    let zoom_popup_sel = zoom_popup.clone();
    let state_zoom_sel = state.clone();
    let transform_zoom_sel = transform.clone();
    let drawing_area_zoom_sel = drawing_area.clone();
    let zoom_level_zoom_sel = zoom_level.clone();
    let canvas_scroller_zoom_sel = canvas_scroller.clone();
    zoom_to_selection_btn.connect_clicked(move |b| {
        let selection_rect = {
            let st = state_zoom_sel.lock().unwrap();
            if let Some(crop_rect) = st.draft_crop_rect().or(st.crop_selection) {
                Some(crop_rect)
            } else if let Some(action_idx) = st.selected_action_index {
                if let Some(action) = st.actions.get(action_idx) {
                    super::super::selection::action_bounds_with_padding(action, 0.0)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(rect) = selection_rect {
            let scroller_w = canvas_scroller_zoom_sel.allocated_width() as f64;
            let scroller_h = canvas_scroller_zoom_sel.allocated_height() as f64;
            let padding = super::canvas::CANVAS_PADDING as f64 * 2.0 + 40.0;
            let available_w = (scroller_w - padding).max(100.0);
            let available_h = (scroller_h - padding).max(100.0);

            let scale_x = available_w / rect.width.max(1) as f64;
            let scale_y = available_h / rect.height.max(1) as f64;
            let new_scale = scale_x.min(scale_y).clamp(0.25, 6.0);

            // Update zoom level and transform
            zoom_level_zoom_sel.set(new_scale);
            {
                let mut t = transform_zoom_sel.lock().unwrap();
                t.scale = new_scale;
                // Center the rect in the view
                t.offset_x =
                    (scroller_w - rect.width as f64 * new_scale) / 2.0 - rect.x as f64 * new_scale;
                t.offset_y =
                    (scroller_h - rect.height as f64 * new_scale) / 2.0 - rect.y as f64 * new_scale;
            }

            drawing_area_zoom_sel.queue_draw();
        }

        if let Some(popover) = b.ancestor(Popover::static_type()) {
            popover.downcast::<Popover>().unwrap().popdown();
        }
        zoom_popup_sel.set_visible(false);
    });

    let scroll_controller = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::DISCRETE,
    );
    let apply_zoom_change_scroll = apply_zoom_change.clone();
    let zoom_level_scroll = zoom_level.clone();
    scroll_controller.connect_scroll(move |_, _dx, dy| {
        if dy < 0.0 {
            apply_zoom_change_scroll(zoom_level_scroll.get() * ZOOM_STEP);
        } else if dy > 0.0 {
            apply_zoom_change_scroll(zoom_level_scroll.get() / ZOOM_STEP);
        }
        glib::Propagation::Stop
    });
    drawing_area.add_controller(scroll_controller);

    let pan_origin = Rc::new(Cell::new((0.0, 0.0)));
    let pan_drag = GestureDrag::new();
    pan_drag.set_button(3);
    let pan_origin_begin = pan_origin.clone();
    let canvas_scroller_begin = canvas_scroller.clone();
    pan_drag.connect_drag_begin(move |_, _x, _y| {
        let hadj = canvas_scroller_begin.hadjustment();
        let vadj = canvas_scroller_begin.vadjustment();
        pan_origin_begin.set((hadj.value(), vadj.value()));
    });
    let pan_origin_update = pan_origin.clone();
    let canvas_scroller_update = canvas_scroller.clone();
    pan_drag.connect_drag_update(move |_, offset_x, offset_y| {
        let hadj = canvas_scroller_update.hadjustment();
        let vadj = canvas_scroller_update.vadjustment();
        let (start_x, start_y) = pan_origin_update.get();
        hadj.set_value((start_x - offset_x).clamp(hadj.lower(), hadj.upper() - hadj.page_size()));
        vadj.set_value((start_y - offset_y).clamp(vadj.lower(), vadj.upper() - vadj.page_size()));
    });
    drawing_area.add_controller(pan_drag);

    let path_copy = path.clone();
    copy_btn.connect_clicked(move |_| {
        if let Err(e) = copy_uri_to_clipboard(&path_copy) {
            eprintln!("Copy failed: {e}");
        }
    });

    let path_upload = path.clone();
    // Prevent concurrent uploads from double-clicks. Worker signals completion
    // back to the GTK main loop so we can re-enable the button (widgets are !Send).
    let uploading = Rc::new(Cell::new(false));
    let (upload_done_tx, upload_done_rx) = std::sync::mpsc::channel::<()>();
    let upload_btn_poll = upload_btn.clone();
    let uploading_poll = uploading.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match upload_done_rx.try_recv() {
            Ok(()) => {
                uploading_poll.set(false);
                upload_btn_poll.set_sensitive(true);
                // Keep listening for subsequent uploads after a failure/retry.
                glib::ControlFlow::Continue
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
    let uploading_click = uploading.clone();
    let upload_btn_click = upload_btn.clone();
    upload_btn.connect_clicked(move |_| {
        if uploading_click.get() {
            return;
        }

        let config = crate::config::load_config();
        if !crate::cloud::upload::is_configured(&config) {
            let (title, body) = crate::cloud::upload::not_configured_notification(&config);
            crate::utils::notify::desktop_notification(title, body);
            return;
        }

        uploading_click.set(true);
        upload_btn_click.set_sensitive(false);

        let path = path_upload.clone();
        let upload_done_tx = upload_done_tx.clone();
        std::thread::spawn(move || {
            match crate::cloud::upload::upload_file(&config, &path) {
                Ok(result) => {
                    if let Err(e) =
                        crate::utils::clipboard::copy_text_to_clipboard(&result.share_url)
                    {
                        eprintln!("Failed to copy share link to clipboard: {e}");
                        crate::utils::notify::desktop_notification(
                            "Upload complete",
                            &format!("Share link: {}", result.share_url),
                        );
                    } else {
                        crate::utils::notify::desktop_notification(
                            "Upload complete",
                            "Share link copied to clipboard",
                        );
                    }
                }
                Err(e) => {
                    crate::utils::notify::desktop_notification("Upload failed", &e.to_string());
                }
            }
            let _ = upload_done_tx.send(());
        });
    });

    let state_box = state.clone();
    let drawing_area_box = drawing_area.downgrade();
    let buttons_box = tool_buttons.clone();
    let apply_crop_btn_box = apply_crop_btn.clone();
    let update_toolbar_for_tool_box = update_toolbar_for_tool.clone();
    let sync_size_control_box = sync_size_control.clone();
    let rebuild_effects_async_box = rebuild_effects_async.clone();
    box_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_box, tool_button_index(Tool::Box));
        if state_box
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Box)
        {
            rebuild_effects_async_box();
        }
        update_toolbar_for_tool_box(Tool::Box);
        sync_size_control_box();
        set_crop_apply_button_state(&apply_crop_btn_box, false, false);
        if let Some(area) = drawing_area_box.upgrade() {
            area.queue_draw();
        }
    });

    let state_circle = state.clone();
    let drawing_area_circle = drawing_area.downgrade();
    let buttons_circle = tool_buttons.clone();
    let apply_crop_btn_circle = apply_crop_btn.clone();
    let update_toolbar_for_tool_circle = update_toolbar_for_tool.clone();
    let sync_size_control_circle = sync_size_control.clone();
    let rebuild_effects_async_circle = rebuild_effects_async.clone();
    circle_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_circle, tool_button_index(Tool::Circle));
        if state_circle
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Circle)
        {
            rebuild_effects_async_circle();
        }
        update_toolbar_for_tool_circle(Tool::Circle);
        sync_size_control_circle();
        set_crop_apply_button_state(&apply_crop_btn_circle, false, false);
        if let Some(area) = drawing_area_circle.upgrade() {
            area.queue_draw();
        }
    });

    let state_text = state.clone();
    let drawing_area_text = drawing_area.downgrade();
    let buttons_text = tool_buttons.clone();
    let apply_crop_btn_text = apply_crop_btn.clone();
    let update_toolbar_for_tool_text = update_toolbar_for_tool.clone();
    let sync_size_control_text = sync_size_control.clone();
    let rebuild_effects_async_text = rebuild_effects_async.clone();
    text_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_text, tool_button_index(Tool::Text));
        if state_text
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Text)
        {
            rebuild_effects_async_text();
        }
        update_toolbar_for_tool_text(Tool::Text);
        sync_size_control_text();
        set_crop_apply_button_state(&apply_crop_btn_text, false, false);
        if let Some(area) = drawing_area_text.upgrade() {
            area.queue_draw();
        }
    });

    let state_obfuscate = state.clone();
    let drawing_area_obfuscate = drawing_area.downgrade();
    let buttons_obfuscate = tool_buttons.clone();
    let apply_crop_btn_obfuscate = apply_crop_btn.clone();
    let update_toolbar_for_tool_obfuscate = update_toolbar_for_tool.clone();
    let sync_size_control_obfuscate = sync_size_control.clone();
    let rebuild_effects_async_obfuscate = rebuild_effects_async.clone();
    obfuscate_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_obfuscate, tool_button_index(Tool::Obfuscate));
        {
            let mut st = state_obfuscate.lock().unwrap();
            let changed = st.set_tool_without_rebuild(Tool::Obfuscate);

            // If the app was backgrounded while an effects rebuild was in-flight, we can end up
            // with the pending flag stuck and no further rebuilds scheduled. Clear it on tool
            // activation and trigger a rebuild if we have any effect actions.
            st.select_effect_rebuild_pending = false;

            // If we changed tool or we have any effect actions, refresh the effect layer.
            let has_effect_actions = st
                .actions
                .iter()
                .any(EditorState::action_requires_effect_rebuild);
            drop(st);

            if changed || has_effect_actions {
                rebuild_effects_async_obfuscate();
            }
        }
        update_toolbar_for_tool_obfuscate(Tool::Obfuscate);
        sync_size_control_obfuscate();
        set_crop_apply_button_state(&apply_crop_btn_obfuscate, false, false);
        if let Some(area) = drawing_area_obfuscate.upgrade() {
            area.queue_draw();
        }
    });

    let state_focus = state.clone();
    let drawing_area_focus = drawing_area.downgrade();
    let buttons_focus = tool_buttons.clone();
    let apply_crop_btn_focus = apply_crop_btn.clone();
    let update_toolbar_for_tool_focus = update_toolbar_for_tool.clone();
    let sync_size_control_focus = sync_size_control.clone();
    let rebuild_effects_async_focus = rebuild_effects_async.clone();
    focus_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_focus, tool_button_index(Tool::Focus));
        if state_focus
            .lock()
            .unwrap()
            .set_tool_without_rebuild(Tool::Focus)
        {
            rebuild_effects_async_focus();
        }
        update_toolbar_for_tool_focus(Tool::Focus);
        sync_size_control_focus();
        set_crop_apply_button_state(&apply_crop_btn_focus, false, false);
        if let Some(area) = drawing_area_focus.upgrade() {
            area.queue_draw();
        }
    });

    let state_number = state.clone();
    let drawing_area_number = drawing_area.downgrade();
    let buttons_number = tool_buttons.clone();
    let apply_crop_btn_number = apply_crop_btn.clone();
    let update_toolbar_for_tool_number = update_toolbar_for_tool.clone();
    let sync_size_control_number = sync_size_control.clone();
    let rebuild_effects_async_number = rebuild_effects_async.clone();
    number_btn.connect_clicked(move |_| {
        let next_tool = {
            let mut st = state_number.lock().unwrap();
            if st.selected_tool == Tool::Number {
                let r = st.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, r)
            } else {
                let r = st.set_tool_without_rebuild(Tool::Number);
                (Tool::Number, r)
            }
        };
        if next_tool.1 {
            rebuild_effects_async_number();
        }

        set_active_tool_button(&buttons_number, tool_button_index(next_tool.0));

        update_toolbar_for_tool_number(next_tool.0);
        sync_size_control_number();
        set_crop_apply_button_state(&apply_crop_btn_number, false, false);
        if let Some(area) = drawing_area_number.upgrade() {
            area.queue_draw();
        }
    });

    let state_highlighter = state.clone();
    let drawing_area_highlighter = drawing_area.downgrade();
    let buttons_highlighter = tool_buttons.clone();
    let apply_crop_btn_highlighter = apply_crop_btn.clone();
    let update_toolbar_for_tool_highlighter = update_toolbar_for_tool.clone();
    let sync_size_control_highlighter = sync_size_control.clone();
    let window_highlighter = window.clone();
    let rebuild_effects_async_highlighter = rebuild_effects_async.clone();
    highlighter_btn.connect_clicked(move |_| {
        let next_tool = {
            let mut st = state_highlighter.lock().unwrap();
            let rebuild = if st.selected_tool == Tool::Highlighter {
                let r = st.set_tool_without_rebuild(Tool::Arrow);
                (Tool::Arrow, r)
            } else {
                let r = st.set_tool_without_rebuild(Tool::Highlighter);
                (Tool::Highlighter, r)
            };
            if rebuild.1 {
                rebuild_effects_async_highlighter();
            }
            rebuild.0
        };

        set_active_tool_button(&buttons_highlighter, tool_button_index(next_tool));
        if !matches!(next_tool, Tool::Highlighter) {
            set_window_cursor_name(&window_highlighter, Some("default"));
        }

        update_toolbar_for_tool_highlighter(next_tool);
        sync_size_control_highlighter();
        set_crop_apply_button_state(&apply_crop_btn_highlighter, false, false);

        if let Some(area) = drawing_area_highlighter.upgrade() {
            area.queue_draw();
        }
    });

    // Wire up pen weight list items for highlighter freehand mode
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

    for weight_list in [pen_weight_list.clone(), highlighter_weight_list.clone()] {
        let mut weight_idx = 0usize;
        let mut child_opt = weight_list.first_child();
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

            let selected_index = weight_idx - 1;
            let state_for_weight = state.clone();
            let drawing_area_weight = drawing_area_for_weight.clone();
            let pen_weight_button_clone = pen_weight_button_for_closure.clone();
            let window_for_weight = window_pen_weight.clone();
            let weight_list_for_sync = weight_list.clone();

            button.connect_clicked(move |b| {
                {
                    let mut st = state_for_weight.lock().unwrap();
                    st.set_pen_weight(weight);
                    let is_highlighter = st.selected_tool == Tool::Highlighter;
                    let is_pen = st.selected_tool == Tool::Pen;
                    if is_highlighter {
                        st.set_highlighter_mode(HighlighterMode::Freehand);
                    }
                    drop(st);

                    if is_pen || is_highlighter {
                        let st = state_for_weight.lock().unwrap();
                        super::cursor::update_pen_cursor(&window_for_weight, &st);
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
            number_start_entry.set_text(&st.numbering_style.format(st.numbering_start));
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
            {
                let mut st = state_style.lock().unwrap();
                st.numbering_style = style;
                st.next_number = st.numbering_start;
            }
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
                st.numbering_start = st.numbering_start.saturating_add(1);
                st.next_number = st.numbering_start;
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
                if st.numbering_start > 1 {
                    st.numbering_start -= 1;
                    st.next_number = st.numbering_start;
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
            {
                let mut st = state_size.lock().unwrap();
                st.number_size = size;
            }

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
        let drawing_area_color = drawing_area.downgrade();
        let color_buttons_group = color_buttons.clone();
        let color_picker_dot_group = color_picker_dot.clone();
        let color_class_names_group = color_class_names.clone();
        let color_popover_group = color_popover.clone();
        let sync_picker_from_color_group = sync_picker_from_color.clone();
        let sync_picker_for_active_tool_group = sync_picker_for_active_tool.clone();
        button.connect_clicked(move |_| {
            let (has_active_text, switched_background) = {
                let mut st = state_color.lock().unwrap();
                let has_active_text = st.active_text_input.is_some();
                let mut switched_background = false;
                if st.selected_tool == Tool::Crop {
                    st.set_crop_background_color(DRAW_COLORS[index]);
                } else if st.selected_tool == Tool::Background {
                    st.background_style = BackgroundStyle::PlainColor(DRAW_COLORS[index]);
                    switched_background = true;
                } else {
                    st.set_color_index(index);
                }
                (has_active_text, switched_background)
            };

            sync_picker_from_color_group(DRAW_COLORS[index]);
            if switched_background {
                sync_picker_for_active_tool_group();
            }

            color_picker::set_active_color_picker_state(
                &color_buttons_group,
                &color_picker_dot_group,
                &color_class_names_group,
                index,
            );
            color_popover_group.popdown();
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

    let state_apply_crop = state.clone();
    let drawing_area_apply_crop = drawing_area.downgrade();
    let apply_crop_btn_click = apply_crop_btn.clone();
    let update_canvas_content_size_apply = update_canvas_content_size.clone();
    let update_crop_size_fields_apply_crop = update_crop_size_fields.clone();
    apply_crop_btn.connect_clicked(move |_| {
        let apply_result = {
            let mut st = state_apply_crop.lock().unwrap();
            st.apply_crop_selection()
        };

        match apply_result {
            Ok(true) => {
                update_canvas_content_size_apply();
                set_crop_apply_button_state(&apply_crop_btn_click, true, false);
                update_crop_size_fields_apply_crop();
                if let Some(area) = drawing_area_apply_crop.upgrade() {
                    area.queue_draw();
                }
            }
            Ok(false) => {
                set_crop_apply_button_state(&apply_crop_btn_click, true, false);
                update_crop_size_fields_apply_crop();
            }
            Err(e) => {
                eprintln!("Failed to apply crop: {e}");
            }
        }
    });

    let state_reset_crop = state.clone();
    let drawing_area_reset_crop = drawing_area.downgrade();
    let update_crop_size_fields_reset_crop = update_crop_size_fields.clone();
    let apply_crop_btn_reset = apply_crop_btn.clone();
    crop_reset_btn.connect_clicked(move |_| {
        {
            let mut st = state_reset_crop.lock().unwrap();
            st.reset_crop_interaction();
        }
        set_crop_apply_button_state(&apply_crop_btn_reset, true, false);
        update_crop_size_fields_reset_crop();
        if let Some(area) = drawing_area_reset_crop.upgrade() {
            area.queue_draw();
        }
    });

    let state_undo = state.clone();
    let drawing_area_undo = drawing_area.downgrade();
    let sync_size_control_undo = sync_size_control.clone();
    let rebuild_effects_async_undo = rebuild_effects_async.clone();
    undo_btn.connect_clicked(move |_| {
        let changed = state_undo.lock().unwrap().undo_without_rebuild();
        if changed {
            rebuild_effects_async_undo();
            sync_size_control_undo();
            if let Some(area) = drawing_area_undo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_redo = state.clone();
    let drawing_area_redo = drawing_area.downgrade();
    let sync_size_control_redo = sync_size_control.clone();
    let rebuild_effects_async_redo = rebuild_effects_async.clone();
    redo_btn.connect_clicked(move |_| {
        let changed = state_redo.lock().unwrap().redo_without_rebuild();
        if changed {
            rebuild_effects_async_redo();
            sync_size_control_redo();
            if let Some(area) = drawing_area_redo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_delete_selected = state.clone();
    let drawing_area_delete_selected = drawing_area.downgrade();
    let rebuild_effects_async_delete = rebuild_effects_async.clone();
    let sync_select_inspector_delete = sync_select_inspector.clone();
    delete_selected_btn.connect_clicked(move |_| {
        if state_delete_selected
            .lock()
            .unwrap()
            .remove_selected_action_without_rebuild()
        {
            rebuild_effects_async_delete();
            sync_select_inspector_delete();
            if let Some(area) = drawing_area_delete_selected.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_save = state.clone();
    let path_save = path.clone();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    save_btn.connect_clicked(move |_| {
        // Hide the editor window immediately so the user gets instant visual
        // feedback when they click Done. Without this, GTK can't repaint until
        // the click handler returns, and the editor stays visible (with the
        // Done button stuck pressed) for the full duration of the save —
        // typically a noticeable freeze when a background tool is in use due
        // to the full-resolution composition + PNG encode that has to run.
        // The actual save work is deferred to an idle callback so this hide
        // gets a chance to render before the heavy work begins.
        if let Some(window) = window_save.upgrade() {
            window.set_visible(false);
        }

        let state_save = state_save.clone();
        let path_save = path_save.clone();
        let window_save = window_save.clone();
        let app_save = app_save.clone();

        glib::idle_add_local_once(move || {
            // Get the state data we need
            let (image_result, annotation_data) = {
                let st = state_save.lock().unwrap();
                let save_result = save_edited_image(&path_save, &st);
                let annotation_result = save_annotations(
                    &path_save,
                    st.base_image.width(),
                    st.base_image.height(),
                    &st.actions,
                    &st.base_image,
                    &st.background_style,
                    st.background_padding,
                    st.background_shadow,
                    st.background_insert,
                    st.auto_balance,
                    st.background_alignment,
                    st.background_corner_radius,
                    st.background_aspect_ratio,
                );
                (save_result, annotation_result)
            };

            // Log annotation errors but don't fail the save
            if let Err(e) = annotation_data {
                eprintln!("[editor] Warning: Failed to save annotations: {e}");
            }

            match image_result {
                Ok(()) => {
                    // Close editor window
                    if let Some(window) = window_save.upgrade() {
                        window.close();
                    }
                    if let Some(app) = app_save.upgrade() {
                        app.quit();
                    }

                    // Show preview via daemon for single-instance coordination
                    // If daemon not running, fall back to spawning directly
                    if !crate::daemon::show_preview_via_daemon(&path_save) {
                        let exe =
                            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
                        if let Err(e) = Command::new(&exe).arg("preview").arg(&path_save).spawn() {
                            eprintln!("[editor] Failed to open preview: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to save edited image: {e}");
                    // Re-show the window so the user can retry, since the file
                    // was never written.
                    if let Some(window) = window_save.upgrade() {
                        window.set_visible(true);
                    }
                }
            }
        });
    });

    let window_close = window.downgrade();
    let app_close = app.downgrade();
    traffic_close.connect_clicked(move |_| {
        if let Some(window) = window_close.upgrade() {
            window.close();
        }
        if let Some(app) = app_close.upgrade() {
            app.quit();
        }
    });

    let drag = GestureDrag::new();
    let drag_last_redraw = Rc::new(Cell::new(0_i64));
    let eyedropper_mode_drag_begin = eyedropper_mode.clone();
    let state_drag_begin = state.clone();
    let transform_drag_begin = transform.clone();
    let drawing_area_begin = drawing_area.downgrade();
    let drag_last_redraw_begin = drag_last_redraw.clone();
    let apply_crop_btn_drag_begin = apply_crop_btn.clone();
    let update_crop_size_fields_drag_begin = update_crop_size_fields.clone();
    let drag_start_transform_begin = drag_start_transform.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        if eyedropper_mode_drag_begin.get() {
            return;
        }

        let t = *transform_drag_begin.lock().unwrap();
        drag_start_transform_begin.borrow_mut().replace(t);
        let view_point = Point { x, y };

        let selected_tool = {
            let st = state_drag_begin.lock().unwrap();
            st.selected_tool
        };
        if !t.contains_view(view_point) && selected_tool != Tool::Crop {
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        let mut st = state_drag_begin.lock().unwrap();

        if st.selected_tool == Tool::Select {
            let image_point = t.view_to_image_clamped(view_point);

            // Check if selected action is an arrow — allow control handle editing.
            let selected_is_arrow = st
                .selected_action_index
                .and_then(|i| st.actions.get(i))
                .map(|a| matches!(a, super::super::types::AnnotationAction::Arrow { .. }))
                .unwrap_or(false);

            if selected_is_arrow {
                // Ensure control_points are initialised for curved/double arrows.
                if let Some(idx) = st.selected_action_index {
                    if let Some(super::super::types::AnnotationAction::Arrow {
                        style,
                        control_points,
                        start,
                        end,
                        ..
                    }) = st.actions.get_mut(idx)
                    {
                        if control_points.is_none() {
                            match style {
                                ArrowStyle::Curved | ArrowStyle::Double => {
                                    let mid = Point {
                                        x: (start.x + end.x) / 2.0,
                                        y: (start.y + end.y) / 2.0,
                                    };
                                    *control_points = Some(vec![*start, mid, *end]);
                                }
                                _ => {
                                    *control_points = Some(vec![*start, *end]);
                                }
                            }
                        }
                    }
                }

                // 1a. Handle hit — check control handles first.
                if let Some(handle_idx) = st.arrow_control_handle_at(image_point) {
                    st.arrow_control_dragging = Some(handle_idx);
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    return;
                }

                // 1b. Body hit — drag the whole arrow; keep handles visible.
                let idx = st.selected_action_index.unwrap();
                let hit_body = super::super::selection::action_contains_point_with_padding(
                    &st.actions[idx],
                    image_point,
                    8.0,
                );
                if hit_body {
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }

            // Generic select drag (non-arrow or click outside arrow).
            st.drag_start_view = Some(view_point);
            st.begin_select_drag_with_scale(t.view_to_image_clamped(view_point), t.scale);
            drop(st);

            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
        }

        // Arrow tool: unified interaction — handle drag, body drag, or new draw.
        if st.selected_tool == Tool::Arrow {
            let image_point = t.view_to_image_clamped(view_point);

            // --- Case 1: an arrow is already selected ---
            let selected_is_arrow = st
                .selected_action_index
                .and_then(|i| st.actions.get(i))
                .map(|a| matches!(a, super::super::types::AnnotationAction::Arrow { .. }))
                .unwrap_or(false);

            if selected_is_arrow {
                // 1a. Handle hit — always check this first regardless of arrow_editing_controls.
                if let Some(handle_idx) = st.arrow_control_handle_at(image_point) {
                    st.arrow_control_dragging = Some(handle_idx);
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    return;
                }

                // 1b. Body hit — drag the whole arrow; keep handles visible.
                let idx = st.selected_action_index.unwrap();
                let hit_body = super::super::selection::action_contains_point_with_padding(
                    &st.actions[idx],
                    image_point,
                    8.0,
                );
                if hit_body {
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.arrow_editing_controls = true; // keep handles visible during move
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }

                // 1c. Clicked outside the selected arrow — deselect, fall through to new draw.
                st.selected_action_index = None;
                st.select_drag_anchor = None;
                st.arrow_editing_controls = false;
            }

            // --- Case 2: no arrow selected — check if click lands on an existing arrow ---
            if st.selected_action_index.is_none()
                && st.select_action_at_point_with_scale(image_point, t.scale)
            {
                let is_arrow = st
                    .selected_action()
                    .map(|a| matches!(a, super::super::types::AnnotationAction::Arrow { .. }))
                    .unwrap_or(false);
                if is_arrow {
                    // Ensure control_points are initialised
                    if let Some(idx) = st.selected_action_index {
                        if let Some(super::super::types::AnnotationAction::Arrow {
                            style,
                            control_points,
                            start,
                            end,
                            ..
                        }) = st.actions.get_mut(idx)
                        {
                            if control_points.is_none() {
                                match style {
                                    ArrowStyle::Curved | ArrowStyle::Double => {
                                        let mid = Point {
                                            x: (start.x + end.x) / 2.0,
                                            y: (start.y + end.y) / 2.0,
                                        };
                                        *control_points = Some(vec![*start, mid, *end]);
                                    }
                                    _ => {
                                        *control_points = Some(vec![*start, *end]);
                                    }
                                }
                            }
                        }
                    }
                    st.arrow_editing_controls = true;
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                } else {
                    // Hit something that isn't an arrow — deselect, fall through to new draw.
                    st.selected_action_index = None;
                    st.select_drag_anchor = None;
                }
            }
        }

        // Text tool with a selected action: check handles first, then fall back to move.
        if st.selected_tool == Tool::Text
            && st.selected_action_index.is_some()
            && st.active_text_input.is_none()
        {
            let image_point = t.view_to_image_clamped(view_point);

            // Compute the committed action's TextEditBounds for handle hit-testing.
            let bounds_opt = if let Some(index) = st.selected_action_index {
                if let Some(super::super::types::AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                }) = st.actions.get(index)
                {
                    let surface =
                        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok();
                    surface
                        .as_ref()
                        .and_then(|s| gtk4::cairo::Context::new(s).ok())
                        .map(|c| {
                            let aw = max_width.unwrap_or_else(|| {
                                (st.base_image.width() as f64 - position.x).max(font.size * 1.8)
                            });
                            super::super::render::text_action_bounds(
                                &c,
                                *position,
                                text,
                                font,
                                Some(aw),
                            )
                        })
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(bounds) = bounds_opt {
                // Hit-test left/right circles.
                let handle_hit = bounds.move_handles.iter().find_map(|(h, center)| {
                    let cv = Point {
                        x: center.x * t.scale + t.offset_x,
                        y: center.y * t.scale + t.offset_y,
                    };
                    let dx = x - cv.x;
                    let dy = y - cv.y;
                    if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                        Some(h.clone())
                    } else {
                        None
                    }
                });
                // Hit-test bottom-right resize box.
                let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, rp)| {
                    let rv = Point {
                        x: rp.x * t.scale + t.offset_x,
                        y: rp.y * t.scale + t.offset_y,
                    };
                    (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                        && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                });

                if handle_hit.is_some() || resize_hit {
                    // Handle drag: set up active_text_is_dragging so the motion
                    // handler takes over — same as the active-edit handle path.
                    st.active_text_bounds = Some(bounds);
                    st.active_text_is_dragging = true;
                    st.active_text_drag_handle = handle_hit;
                    st.active_text_drag_start = Some(image_point);
                    st.active_text_drag_start_bounds =
                        st.active_text_bounds.as_ref().map(|b| b.rect);
                    st.active_text_is_resizing = resize_hit;
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }

            // No handle hit — move the whole action.
            st.drag_start_view = Some(view_point);
            st.select_drag_anchor = Some(image_point);
            st.select_resize_handle = None;
            drop(st);
            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
        }

        if matches!(st.selected_tool, Tool::Text | Tool::Number) {
            return;
        }

        if st.selected_tool == Tool::Crop {
            let image_point = t.view_to_image(view_point);
            st.drag_start_view = Some(view_point);
            if st.begin_crop_drag_with_scale(image_point, t.scale) {
                let has_selection = st.crop_selection.is_some();
                drop(st);
                set_crop_apply_button_state(&apply_crop_btn_drag_begin, true, has_selection);
                update_crop_size_fields_drag_begin();
                if let Some(area) = drawing_area_begin.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_begin.set(glib::monotonic_time());
                return;
            }

            st.drag_shift_active = shift_pressed;
            st.begin_drag(image_point);
            st.crop_selection = None;
            drop(st);
            set_crop_apply_button_state(&apply_crop_btn_drag_begin, true, false);
            update_crop_size_fields_drag_begin();
            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
        }

        // Box/Circle tool: unified interaction — resize, move, or draw new.
        if matches!(st.selected_tool, Tool::Box | Tool::Circle) {
            let image_point = t.view_to_image_clamped(view_point);

            // If an action is already selected and we're dragging it, continue.
            if st.selected_action_index.is_some() && st.select_drag_anchor.is_some() {
                drop(st);
                if let Some(area) = drawing_area_begin.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_begin.set(glib::monotonic_time());
                return;
            }

            // If an action is already selected, check resize handles first, then body hit.
            if st.selected_action_index.is_some() {
                if let Some(index) = st.selected_action_index {
                    if let Some(selected) = st.actions.get(index) {
                        let is_matching_type = match selected {
                            super::super::types::AnnotationAction::Box { .. } => {
                                st.selected_tool == Tool::Box
                            }
                            super::super::types::AnnotationAction::Circle { .. } => {
                                st.selected_tool == Tool::Circle
                            }
                            _ => false,
                        };
                        if is_matching_type {
                            // Check resize handles first.
                            let handle_hit_radius =
                                super::super::color::selection_handle_hit_radius_for_scale(t.scale);
                            if let Some(handle) =
                                super::super::selection::action_resize_handle_at_point_with_radius(
                                    selected,
                                    image_point,
                                    handle_hit_radius,
                                )
                            {
                                st.select_resize_handle = Some(handle);
                                st.select_drag_anchor = Some(image_point);
                                st.drag_start_view = Some(view_point);
                                drop(st);
                                if let Some(area) = drawing_area_begin.upgrade() {
                                    area.queue_draw();
                                }
                                drag_last_redraw_begin.set(glib::monotonic_time());
                                return;
                            }

                            // Body hit — move the whole action.
                            let hit_padding =
                                super::super::color::selection_hit_padding_for_scale(t.scale);
                            if super::super::selection::action_contains_point_with_padding(
                                selected,
                                image_point,
                                hit_padding,
                            ) {
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                                st.drag_start_view = Some(view_point);
                                drop(st);
                                if let Some(area) = drawing_area_begin.upgrade() {
                                    area.queue_draw();
                                }
                                drag_last_redraw_begin.set(glib::monotonic_time());
                                return;
                            }
                        }
                    }
                }
                // Clicked outside the selected action — deselect, fall through to new draw.
                st.selected_action_index = None;
                st.select_drag_anchor = None;
            }

            // No action selected — check if click lands on an existing matching action.
            if st.selected_action_index.is_none() {
                let hit_padding = super::super::color::selection_hit_padding_for_scale(t.scale);
                let hit_index = st
                    .actions
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, action)| {
                        let is_matching_type = match action {
                            super::super::types::AnnotationAction::Box { .. } => {
                                st.selected_tool == Tool::Box
                            }
                            super::super::types::AnnotationAction::Circle { .. } => {
                                st.selected_tool == Tool::Circle
                            }
                            _ => false,
                        };
                        is_matching_type
                            && super::super::selection::action_contains_point_with_padding(
                                action,
                                image_point,
                                hit_padding,
                            )
                    })
                    .map(|(index, _)| index);

                if let Some(index) = hit_index {
                    st.selected_action_index = Some(index);
                    // Check resize handles on the newly selected action.
                    let handle_hit_radius =
                        super::super::color::selection_handle_hit_radius_for_scale(t.scale);
                    if let Some(handle) =
                        super::super::selection::action_resize_handle_at_point_with_radius(
                            &st.actions[index],
                            image_point,
                            handle_hit_radius,
                        )
                    {
                        st.select_resize_handle = Some(handle);
                    } else {
                        st.select_resize_handle = None;
                    }
                    st.select_drag_anchor = Some(image_point);
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }
            // No hit — fall through to normal draw.
        }

        st.drag_shift_active = shift_pressed;
        st.begin_drag(t.view_to_image_clamped(view_point));
        st.drag_start_view = Some(view_point);
        drop(st);

        if let Some(area) = drawing_area_begin.upgrade() {
            area.queue_draw();
        }
        drag_last_redraw_begin.set(glib::monotonic_time());
    });

    let eyedropper_mode_drag_update = eyedropper_mode.clone();
    let state_drag_update = state.clone();
    let transform_drag_update = transform.clone();
    let drawing_area_update = drawing_area.downgrade();
    let drag_last_redraw_update = drag_last_redraw.clone();
    let update_crop_size_fields_drag_update = update_crop_size_fields.clone();
    let rebuild_effects_async_drag_update = rebuild_effects_async.clone();
    let drag_start_transform_update = drag_start_transform.clone();
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if eyedropper_mode_drag_update.get() {
            return;
        }

        let t = drag_start_transform_update
            .borrow()
            .unwrap_or_else(|| *transform_drag_update.lock().unwrap());
        let mut st = state_drag_update.lock().unwrap();

        // Arrow control point dragging
        if let Some(handle_idx) = st.arrow_control_dragging {
            let start_view = st.drag_start_view.unwrap_or(Point { x: 0.0, y: 0.0 });
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };
            let image_point = if handle_idx == 1 {
                t.view_to_image(current_view)
            } else {
                t.view_to_image_clamped(current_view)
            };
            st.move_arrow_control_handle(handle_idx, image_point);
            drop(st);
            if let Some(area) = drawing_area_update.upgrade() {
                area.queue_draw();
            }
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        // Text tool handle drag: the motion handler handles updates via raw motion events.
        // Just skip drag_update for handle drags — don't interfere.
        if st.selected_tool == Tool::Text
            && st.active_text_input.is_none()
            && st.active_text_is_dragging
        {
            return;
        }

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.selected_action_index.is_some()
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(st.selected_tool, Tool::Box | Tool::Circle)
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some())
            {
                let now = glib::monotonic_time();
                if now - drag_last_redraw_update.get() < DRAG_REDRAW_INTERVAL_US {
                    return;
                }

                let moved = st.update_select_drag(t.view_to_image_clamped(current_view));
                // Check if we moved/resized an effect action (obfuscate/focus).
                // If so, trigger a real-time async rebuild so the effect updates
                // during the drag rather than only on release.
                // Clear the dirty flag here so we don't re-schedule on every
                // drag tick — the coalescing in rebuild_effects_async handles
                // the case where a rebuild is already in-flight.
                let needs_effect_rebuild = st.select_drag_effect_dirty;
                if needs_effect_rebuild {
                    st.select_drag_effect_dirty = false;
                }
                drag_last_redraw_update.set(now);
                drop(st);
                if moved {
                    if needs_effect_rebuild {
                        rebuild_effects_async_drag_update();
                    }
                    if let Some(area) = drawing_area_update.upgrade() {
                        area.queue_draw();
                    }
                }
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number)
                && !(st.selected_tool == Tool::Text
                    && st.selected_action_index.is_some()
                    && st.active_text_input.is_none())
            {
                return;
            }

            if st.selected_tool == Tool::Crop {
                let now = glib::monotonic_time();
                if now - drag_last_redraw_update.get() < DRAG_REDRAW_INTERVAL_US {
                    return;
                }

                let image_point = t.view_to_image(current_view);
                if st.select_drag_anchor.is_some() {
                    st.update_crop_drag(image_point);
                } else {
                    st.drag_shift_active = shift_pressed;
                    st.update_drag(image_point);
                }
                drag_last_redraw_update.set(now);
                drop(st);
                update_crop_size_fields_drag_update();
                if let Some(area) = drawing_area_update.upgrade() {
                    area.queue_draw();
                }
                return;
            }

            if !t.contains_view(current_view) {
                return;
            }

            st.drag_shift_active = shift_pressed;
            st.update_drag(t.view_to_image(current_view));
            drop(st);
            let now = glib::monotonic_time();
            if now - drag_last_redraw_update.get() >= DRAG_REDRAW_INTERVAL_US {
                drag_last_redraw_update.set(now);
                if let Some(area) = drawing_area_update.upgrade() {
                    area.queue_draw();
                }
            }
        }
    });

    let eyedropper_mode_drag_end = eyedropper_mode.clone();
    let state_drag_end = state.clone();
    let transform_drag_end = transform.clone();
    let drawing_area_end = drawing_area.downgrade();
    let drag_last_redraw_end = drag_last_redraw.clone();
    let apply_crop_btn_drag_end = apply_crop_btn.clone();
    let update_crop_size_fields_drag_end = update_crop_size_fields.clone();
    let sync_size_control_drag_end = sync_size_control.clone();
    let sync_select_inspector_drag_end = sync_select_inspector.clone();
    let rebuild_effects_async_drag_end = rebuild_effects_async.clone();
    drag.connect_drag_end(move |gesture, offset_x, offset_y| {
        if eyedropper_mode_drag_end.get() {
            return;
        }

        let t = *transform_drag_end.lock().unwrap();
        let mut st = state_drag_end.lock().unwrap();

        // Arrow control point dragging: clear and return
        if st.arrow_control_dragging.is_some() {
            st.finalize_arrow_interaction_cleanup();
            drop(st);
            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(st.selected_tool, Tool::Box | Tool::Circle)
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some())
            {
                st.update_select_drag(t.view_to_image_clamped(current_view));
                if st.end_select_drag_without_rebuild_and_check_effect() {
                    rebuild_effects_async_drag_end.clone()();
                }
                drop(st);

                sync_size_control_drag_end();
                sync_select_inspector_drag_end();
                if let Some(area) = drawing_area_end.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_end.set(glib::monotonic_time());
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }

            if st.selected_tool == Tool::Arrow
                && st.selected_action_index.is_none()
                && offset_x.hypot(offset_y) < ARROW_CLICK_NOOP_DISTANCE
            {
                st.finalize_arrow_interaction_cleanup();
                drop(st);
                if let Some(area) = drawing_area_end.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_end.set(glib::monotonic_time());
                return;
            }

            let mut crop_selection_ready = None;
            if st.selected_tool == Tool::Crop {
                let image_point = t.view_to_image(current_view);
                if st.select_drag_anchor.is_some() {
                    st.update_crop_drag(image_point);
                    crop_selection_ready = Some(st.crop_selection.is_some());
                    st.end_crop_drag();
                } else {
                    st.drag_shift_active = shift_pressed;
                    st.update_drag(image_point);
                    st.crop_selection = st.draft_crop_rect();
                    crop_selection_ready = Some(st.crop_selection.is_some());
                    st.clear_drag();
                }
                drop(st);
            } else if let Some(action) = st.finalize_drag_action() {
                // Check if this action requires async effect rebuild
                let needs_async_rebuild = EditorState::action_requires_effect_rebuild(&action);
                st.push_action(action);
                drop(st);
                if needs_async_rebuild {
                    rebuild_effects_async_drag_end.clone()();
                }
            } else {
                st.clear_drag();
                drop(st); // MUST drop before calling sync_size_control which also locks state
            }

            sync_size_control_drag_end();

            if let Some(has_selection) = crop_selection_ready {
                set_crop_apply_button_state(&apply_crop_btn_drag_end, true, has_selection);
            }
            update_crop_size_fields_drag_end();

            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_end.set(glib::monotonic_time());
        }
    });
    drawing_area.add_controller(drag);

    let key_controller = EventControllerKey::new();
    let state_key = state.clone();
    let drawing_area_key = drawing_area.downgrade();

    key_controller.connect_key_pressed(move |_, key, _, _| {
        let keyval = key;

        if keyval == gdk::Key::Escape {
            let has_active_edit = state_key.lock().unwrap().active_text_bounds.is_some();
            if has_active_edit {
                state_key.lock().unwrap().cancel_text_edit();
                if let Some(area) = drawing_area_key.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }
        }

        glib::Propagation::Proceed
    });

    drawing_area.add_controller(key_controller);

    let click = GestureClick::new();
    click.set_button(1);
    let window_click = window.clone();
    let state_click = state.clone();
    let transform_click = transform.clone();
    let drawing_area_click = drawing_area.downgrade();
    let color_buttons_click = color_buttons.clone();
    let color_picker_dot_click = color_picker_dot.clone();
    let color_class_names_click = color_class_names.clone();
    let eyedropper_mode_click = eyedropper_mode.clone();
    let eyedropper_from_sidebar_click = eyedropper_from_sidebar.clone();
    let eyedropper_point_click = eyedropper_point.clone();
    let eyedropper_rendered_click = eyedropper_rendered.clone();
    let color_popover_canvas_click = color_popover.clone();
    let set_picker_panel_visibility_canvas_click = set_picker_panel_visibility.clone();
    let canvas_eyedropper_ring_click = canvas_eyedropper_ring.clone();
    let apply_picker_color_to_editor_canvas_click = apply_picker_color_to_editor.clone();
    let sync_picker_from_color_canvas_click = sync_picker_from_color.clone();
    let add_color_to_custom_slots_click = add_color_to_custom_slots.clone();
    let sync_size_control_canvas_click = sync_size_control.clone();
    let text_size_label_click = text_size_label.clone();
    let font_family_label_click = font_family_label.clone();
    let text_size_list_click = text_size_list.clone();
    let font_family_list_click = font_family_list.clone();
    let sync_select_inspector_canvas_click = sync_select_inspector.clone();
    click.connect_pressed(move |_gesture, n_press, x, y| {
        let t = *transform_click.lock().unwrap();
        let view_point = Point { x, y };

        let text_hit = {
            let st = state_click.lock().unwrap();
            st.active_text_bounds.as_ref().map(|bounds| {
                let click_image = t.view_to_image_clamped(view_point);
                let inside_bounds = click_image.x >= bounds.rect.x as f64
                    && click_image.x <= (bounds.rect.x + bounds.rect.width) as f64
                    && click_image.y >= bounds.rect.y as f64
                    && click_image.y <= (bounds.rect.y + bounds.rect.height) as f64;

                let handle_hit = bounds.move_handles.iter().find_map(|(handle, center)| {
                    let center_view = Point {
                        x: center.x * t.scale + t.offset_x,
                        y: center.y * t.scale + t.offset_y,
                    };
                    let dx = x - center_view.x;
                    let dy = y - center_view.y;
                    if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                        Some(handle.clone())
                    } else {
                        None
                    }
                });

                let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, resize_pos)| {
                    let resize_view = Point {
                        x: resize_pos.x * t.scale + t.offset_x,
                        y: resize_pos.y * t.scale + t.offset_y,
                    };
                    let dx = x - resize_view.x;
                    let dy = y - resize_view.y;
                    dx.abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5 && dy.abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                });

                (click_image, inside_bounds, handle_hit, resize_hit)
            })
        };

        if let Some((click_image, inside_bounds, handle_hit, resize_hit)) = text_hit {
            if let Some(handle) = handle_hit {
                let mut st = state_click.lock().unwrap();
                st.active_text_is_dragging = true;
                st.active_text_drag_handle = Some(handle);
                st.active_text_drag_start = Some(click_image);
                st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
                st.active_text_is_resizing = false;
                st.reset_text_cursor_blink();
                return;
            }

            if resize_hit {
                let mut st = state_click.lock().unwrap();
                st.active_text_is_dragging = true;
                st.active_text_drag_handle = None;
                st.active_text_drag_start = Some(click_image);
                st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
                st.active_text_is_resizing = true;
                st.reset_text_cursor_blink();
                return;
            }

            if inside_bounds {
                let mut st = state_click.lock().unwrap();
                if let Some(input) = st.active_text_input.as_ref() {
                    let surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1)
                        .expect("create caret hit-test surface");
                    let context = gtk4::cairo::Context::new(&surface)
                        .expect("create caret hit-test context");
                    let font = FontSettings {
                        family: st.text_font_family.clone(),
                        size: st.text_size,
                        style: FontStyle::Normal,
                        decoration: TextDecoration::None,
                        alignment: TextAlignment::Left,
                    };
                    let cursor_position = cursor_position_for_text_point(
                        &context,
                        st.active_text_bounds.as_ref().unwrap(),
                        &input.text,
                        &font,
                        click_image,
                    );
                    st.set_text_cursor_position(cursor_position);
                } else {
                    st.reset_text_cursor_blink();
                }
                if let Some(area) = drawing_area_click.upgrade() {
                    area.grab_focus();
                    area.queue_draw();
                }
                return;
            }

            {
                let mut st = state_click.lock().unwrap();
                if let Some(action) = st.commit_text_input() {
                    st.push_action(action);
                }
            }
            if let Some(area) = drawing_area_click.upgrade() {
                area.queue_draw();
            }
        }

        if eyedropper_mode_click.get() {
            if !t.contains_view(view_point) {
                return;
            }

            let image_point = t.view_to_image_clamped(view_point);
            let picked_color = {
                let rendered = eyedropper_rendered_click.borrow();
                if let Some(rendered) = rendered.as_ref() {
                    sample_rendered_color_at_point(rendered, image_point)
                } else {
                    let st = state_click.lock().unwrap();
                    sample_editor_color_at_point(&st, image_point)
                }
            };

            let mut reopen_color_popover = false;
            let from_sidebar = eyedropper_from_sidebar_click.get();
            if let Some(color) = picked_color {
                // Only add to custom colors when picked from sidebar
                add_color_to_custom_slots_click(color);
                if !from_sidebar {
                    // Only apply to editor and sync picker if not from sidebar
                    apply_picker_color_to_editor_canvas_click(color);
                    sync_picker_from_color_canvas_click(color);
                    reopen_color_popover = true;
                }
            }

            eyedropper_mode_click.set(false);
            eyedropper_from_sidebar_click.set(false);
            *eyedropper_point_click.borrow_mut() = None;
            *eyedropper_rendered_click.borrow_mut() = None;
            canvas_eyedropper_ring_click.set_visible(false);
            set_window_cursor_name(&window_click, None);

            if reopen_color_popover {
                set_picker_panel_visibility_canvas_click(true);
                color_popover_canvas_click.popup();
            }

            if let Some(area) = drawing_area_click.upgrade() {
                area.queue_draw();
            }
            return;
        }

        if !t.contains_view(view_point) {
            return;
        }

        let image_point = t.view_to_image_clamped(view_point);
        let selected_tool = state_click.lock().unwrap().selected_tool;

        match selected_tool {
            Tool::Select => {
                let (selected_color_index, selected_text_size, selected_font_family, began_reedit) = {
                    let mut st = state_click.lock().unwrap();
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }
                    st.select_action_at_point_with_scale(image_point, t.scale);

                    // Ensure control_points are initialised for selected arrows.
                    if let Some(idx) = st.selected_action_index {
                        if let Some(super::super::types::AnnotationAction::Arrow {
                            style,
                            control_points,
                            start,
                            end,
                            ..
                        }) = st.actions.get_mut(idx)
                        {
                            if control_points.is_none() {
                                match style {
                                    ArrowStyle::Curved | ArrowStyle::Double => {
                                        let mid = Point {
                                            x: (start.x + end.x) / 2.0,
                                            y: (start.y + end.y) / 2.0,
                                        };
                                        *control_points = Some(vec![*start, mid, *end]);
                                    }
                                    _ => {
                                        *control_points = Some(vec![*start, *end]);
                                    }
                                }
                            }
                            st.arrow_editing_controls = true;
                        } else {
                            st.arrow_editing_controls = false;
                        }
                    }

                    let mut began_reedit = false;
                    if n_press >= 2 {
                        began_reedit = st.begin_editing_selected_text();
                    }
                    let selected_color = if began_reedit {
                        st.get_text_input().map(|input| input.color)
                    } else {
                        st.selected_action_color()
                    };
                    if let Some(color) = selected_color {
                        st.selected_color = color;
                    }
                    if let Some(text_size) = st.selected_text_action_size() {
                        st.text_size = text_size;
                    }
                    if let Some(stroke_size) = st.selected_action_stroke_size() {
                        st.stroke_size = stroke_size;
                    }
                    if let Some(font_family) = st.selected_text_font_family() {
                        st.text_font_family = font_family;
                    }

                    let selected_color_index = selected_color.map(palette_index_for_color);
                    let selected_text_size = Some(st.text_size);
                    let selected_font_family = Some(st.text_font_family.clone());
                    (selected_color_index, selected_text_size, selected_font_family, began_reedit)
                };

                sync_size_control_canvas_click();
                sync_select_inspector_canvas_click();
                if let Some(size) = selected_text_size {
                    text_size_label_click.set_label(&format!("{}pt", size as i32));
                    sync_text_option_selection(
                        &text_size_list_click,
                        TEXT_SIZE_OPTIONS
                            .iter()
                            .position(|candidate| *candidate == size as i32),
                    );
                }
                if let Some(family) = selected_font_family {
                    font_family_label_click.set_label(&family);
                    sync_text_option_selection(
                        &font_family_list_click,
                        TEXT_FONT_FAMILIES
                            .iter()
                            .position(|candidate| *candidate == family.as_str()),
                    );
                }

                if let Some(index) = selected_color_index {
                    color_picker::clear_active_color_picker_palette_state(&color_buttons_click);
                    color_picker::set_color_picker_trigger_dot_state(
                        &color_picker_dot_click,
                        &color_class_names_click,
                        index,
                    );
                }

                if let Some(area) = drawing_area_click.upgrade() {
                    if began_reedit {
                        area.grab_focus();
                    }
                    area.queue_draw();
                }
            }
            Tool::Text => {
                let (text_size, font_family) = {
                    let mut st = state_click.lock().unwrap();

                    // Commit any active text input first.
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }

                    // Check if the click lands on an existing text action.
                    let hit_index = st.actions.iter().enumerate().rev().find_map(|(index, action)| {
                        if matches!(action, super::super::types::AnnotationAction::Text { .. })
                            && super::super::selection::action_contains_point_with_padding(action, image_point, 0.0)
                        {
                            Some(index)
                        } else {
                            None
                        }
                    });

                    if let Some(index) = hit_index {
                        // Select the action and sync color/size state.
                        st.selected_action_index = Some(index);
                        if let Some(color) = st.selected_action_color() {
                            st.selected_color = color;
                        }
                        if let Some(sz) = st.selected_text_action_size() {
                            st.text_size = sz;
                        }
                        if let Some(fam) = st.selected_text_font_family() {
                            st.text_font_family = fam;
                        }

                        if n_press >= 2 {
                            // Double-click: begin re-editing.
                            st.begin_editing_selected_text();
                        } else {
                            // Single-click: first check if the click is on a
                            // TextEditBounds handle (circles / resize box).
                            // If yes → active_text_is_dragging path (motion handler).
                            // If no  → select_drag_anchor path (GestureDrag move).
                            let bounds_opt = if let Some(
                                super::super::types::AnnotationAction::Text {
                                    position, text, font, max_width, ..
                                }
                            ) = st.actions.get(index) {
                                let surface = gtk4::cairo::ImageSurface::create(
                                    gtk4::cairo::Format::ARgb32, 1, 1,
                                ).ok();
                                surface.as_ref()
                                    .and_then(|s| gtk4::cairo::Context::new(s).ok())
                                    .map(|c| {
                                        let aw = max_width.unwrap_or_else(|| {
                                            (st.base_image.width() as f64 - position.x)
                                                .max(font.size * 1.8)
                                        });
                                        super::super::render::text_action_bounds(
                                            &c, *position, text, font, Some(aw),
                                        )
                                    })
                            } else { None };

                            let mut handle_drag_started = false;
                            if let Some(bounds) = bounds_opt {
                                let handle_hit = bounds.move_handles.iter().find_map(|(h, center)| {
                                    let cv = Point {
                                        x: center.x * t.scale + t.offset_x,
                                        y: center.y * t.scale + t.offset_y,
                                    };
                                    let dx = x - cv.x;
                                    let dy = y - cv.y;
                                    if (dx*dx + dy*dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                                        Some(h.clone())
                                    } else { None }
                                });
                                let resize_hit = bounds.resize_handle.as_ref().is_some_and(
                                    |(_, rp)| {
                                        let rv = Point {
                                            x: rp.x * t.scale + t.offset_x,
                                            y: rp.y * t.scale + t.offset_y,
                                        };
                                        (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                            && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                    }
                                );

                                if handle_hit.is_some() || resize_hit {
                                    // Set up exactly like the active-edit handle path.
                                    // The motion handler and click_released handle the rest.
                                    st.active_text_bounds = Some(bounds);
                                    st.active_text_is_dragging = true;
                                    st.active_text_drag_handle = handle_hit;
                                    st.active_text_drag_start = Some(image_point);
                                    st.active_text_drag_start_bounds =
                                        st.active_text_bounds.as_ref().map(|b| b.rect);
                                    st.active_text_is_resizing = resize_hit;
                                    handle_drag_started = true;
                                }
                            }

                            if !handle_drag_started {
                                // No handle hit — set anchor for GestureDrag move.
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                            }
                        }
                    } else {
                        // Click on empty area: deselect and start a new text box.
                        st.selected_action_index = None;
                        let initial_width = (st.text_size * 1.8).max(140.0);
                        let initial_height = (st.text_size * 1.45 + 16.0).max(44.0);
                        st.begin_text_input(image_point, initial_width, initial_height);
                    }

                    (st.text_size, st.text_font_family.clone())
                };

                text_size_label_click.set_label(&format!("{}pt", text_size as i32));
                font_family_label_click.set_label(&font_family);
                sync_text_option_selection(
                    &text_size_list_click,
                    TEXT_SIZE_OPTIONS
                        .iter()
                        .position(|candidate| *candidate == text_size as i32),
                );
                sync_text_option_selection(
                    &font_family_list_click,
                    TEXT_FONT_FAMILIES
                        .iter()
                        .position(|candidate| *candidate == font_family.as_str()),
                );

                if let Some(area) = drawing_area_click.upgrade() {
                    area.grab_focus();
                    area.queue_draw();
                }
            }
            Tool::Number => {
                state_click.lock().unwrap().add_number_marker(image_point);
                sync_size_control_canvas_click();
                if let Some(area) = drawing_area_click.upgrade() {
                    area.queue_draw();
                }
            }
            _ => {}
        }
    });

    let state_release = state.clone();
    let drawing_area_release = drawing_area.downgrade();
    click.connect_released(move |_gesture, _n_press, _x, _y| {
        let should_refocus = {
            let mut st = state_release.lock().unwrap();
            if st.active_text_is_dragging {
                let was_resizing = st.active_text_is_resizing;
                st.active_text_is_dragging = false;
                st.active_text_drag_handle = None;
                st.active_text_drag_start = None;
                st.active_text_drag_start_bounds = None;
                st.active_text_is_resizing = false;

                if st.active_text_input.is_some() {
                    // Active edit session: reflow text to fit new bounds.
                    if was_resizing {
                        st.fit_active_text_to_layout_preserving_box();
                    } else {
                        st.fit_active_text_to_layout_preserving_font_size();
                    }
                    true // refocus for typing
                } else if let (Some(bounds), Some(index)) =
                    (st.active_text_bounds.take(), st.selected_action_index)
                {
                    // Committed action handle resize: write new bounds back.
                    if let Some(super::super::types::AnnotationAction::Text {
                        position,
                        font,
                        max_width,
                        ..
                    }) = st.actions.get_mut(index)
                    {
                        let padding_y = 8.0;
                        position.x = bounds.rect.x as f64;
                        position.y = bounds.rect.y as f64 + font.size + padding_y;
                        *max_width = Some(bounds.rect.width as f64);
                    }
                    st.redo_actions.clear();
                    false
                } else {
                    false
                }
            } else {
                false
            }
        };
        if let Some(area) = drawing_area_release.upgrade() {
            if should_refocus {
                area.grab_focus();
            }
            area.queue_draw();
        }
    });

    drawing_area.add_controller(click);

    let motion = EventControllerMotion::new();
    let eyedropper_mode_motion = eyedropper_mode.clone();
    let eyedropper_point_motion = eyedropper_point.clone();
    let canvas_eyedropper_ring_motion = canvas_eyedropper_ring.clone();
    let state_motion = state.clone();
    let transform_motion = transform.clone();
    let window_motion = window.downgrade();
    let drawing_area_motion = drawing_area.downgrade();
    motion.connect_motion(move |_, x, y| {
        let t = *transform_motion.lock().unwrap();
        let view_point = Point { x, y };

        if eyedropper_mode_motion.get() {
            if !t.contains_view(view_point) {
                *eyedropper_point_motion.borrow_mut() = None;
                canvas_eyedropper_ring_motion.set_visible(false);
                if let Some(window) = window_motion.upgrade() {
                    set_window_cursor_name(&window, Some("crosshair"));
                }
                return;
            }

            *eyedropper_point_motion.borrow_mut() = Some(t.view_to_image_clamped(view_point));
            canvas_eyedropper_ring_motion.set_visible(true);
            let (left, top) = eyedropper_loupe_position(x, y);
            canvas_eyedropper_ring_motion.set_margin_start(left);
            canvas_eyedropper_ring_motion.set_margin_top(top);
            canvas_eyedropper_ring_motion.queue_draw();

            if let Some(window) = window_motion.upgrade() {
                set_window_cursor_name(&window, Some("none"));
            }
            return;
        }

        let is_highlighter = {
            let st = state_motion.lock().unwrap();
            st.selected_tool == Tool::Highlighter
        };

        let is_pen = {
            let st = state_motion.lock().unwrap();
            st.selected_tool == Tool::Pen
        };

        if is_highlighter {
            if let Some(window) = window_motion.upgrade() {
                if !t.contains_view(view_point) {
                    set_window_cursor_name(&window, Some("pointer"));
                } else {
                    let st = state_motion.lock().unwrap();
                    let image_point = t.view_to_image_clamped(view_point);
                    super::cursor::update_cursor_for_position(&window, &st, image_point, t.scale);
                }
            }
        } else if is_pen {
            if let Some(window) = window_motion.upgrade() {
                if !t.contains_view(view_point) {
                    set_window_cursor_name(&window, Some("pointer"));
                } else {
                    let st = state_motion.lock().unwrap();
                    super::cursor::update_pen_cursor(&window, &st);
                }
            }
        } else {
            let cursor_name = {
                let st = state_motion.lock().unwrap();
                cursor_name_for_view_point(&st, t, view_point)
            };

            if let Some(window) = window_motion.upgrade() {
                set_window_cursor_name(&window, Some(cursor_name));
            }
        }

        // In Text tool mode: detect hover over existing text actions.
        // Show outline border on hover and change cursor to "grab".
        {
            let mut st = state_motion.lock().unwrap();
            if st.selected_tool == Tool::Text && st.active_text_input.is_none() {
                let image_point = t.view_to_image_clamped(view_point);
                let hit = st
                    .actions
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(index, action)| {
                        if matches!(action, super::super::types::AnnotationAction::Text { .. })
                            && super::super::selection::action_contains_point_with_padding(
                                action,
                                image_point,
                                0.0,
                            )
                        {
                            Some(index)
                        } else {
                            None
                        }
                    });
                if st.hovered_text_action_index != hit {
                    st.hovered_text_action_index = hit;
                    if let Some(area) = drawing_area_motion.upgrade() {
                        area.queue_draw();
                    }
                }
                if hit.is_some() {
                    if let Some(window) = window_motion.upgrade() {
                        set_window_cursor_name(&window, Some("grab"));
                    }
                }
            } else if st.selected_tool != Tool::Text && st.hovered_text_action_index.is_some() {
                st.hovered_text_action_index = None;
                if let Some(area) = drawing_area_motion.upgrade() {
                    area.queue_draw();
                }
            }
        }

        // Check for text edit handle hover
        let text_bounds = state_motion.lock().unwrap().active_text_bounds.clone();
        if let Some(bounds) = &text_bounds {
            let t = *transform_motion.lock().unwrap();
            let view_point = Point { x, y };
            let _image_point = t.view_to_image(view_point);

            // Check move handles (convert to view coordinates)
            for (_handle, center) in &bounds.move_handles {
                let center_view = Point {
                    x: center.x * t.scale + t.offset_x,
                    y: center.y * t.scale + t.offset_y,
                };
                let dx = x - center_view.x;
                let dy = y - center_view.y;
                if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS {
                    if let Some(window) = window_motion.upgrade() {
                        set_window_cursor_name(&window, Some("grab"));
                    }
                    return;
                }
            }

            // Check resize handle
            if let Some((_, resize_pos)) = &bounds.resize_handle {
                let resize_view = Point {
                    x: resize_pos.x * t.scale + t.offset_x,
                    y: resize_pos.y * t.scale + t.offset_y,
                };
                let dx = x - resize_view.x;
                let dy = y - resize_view.y;
                if dx.abs() < RESIZE_HANDLE_DRAG_SIZE && dy.abs() < RESIZE_HANDLE_DRAG_SIZE {
                    if let Some(window) = window_motion.upgrade() {
                        set_window_cursor_name(&window, Some("nwse-resize"));
                    }
                    return;
                }
            }
        }

        let drag_state = {
            let st = state_motion.lock().unwrap();
            if st.active_text_is_dragging {
                st.active_text_drag_start.map(|start| {
                    (
                        start,
                        st.active_text_drag_handle.clone(),
                        st.active_text_drag_start_bounds,
                        st.active_text_is_resizing,
                        st.base_image.width() as i32,
                        st.base_image.height() as i32,
                    )
                })
            } else {
                None
            }
        };
        if let Some((start_point, handle, start_bounds, is_resizing, image_width, image_height)) =
            drag_state
        {
            let view_point = Point { x, y };
            let current_point = t.view_to_image(view_point);
            let dx = current_point.x - start_point.x;
            let dy = current_point.y - start_point.y;

            {
                let mut st = state_motion.lock().unwrap();
                // Compute min_width before the mutable borrow of active_text_bounds.
                let min_width = if st.active_text_input.is_none() && !is_resizing {
                    st.committed_text_min_width()
                } else {
                    50.0
                };
                if let (Some(bounds), Some(start_bounds)) =
                    (st.active_text_bounds.as_mut(), start_bounds)
                {
                    let min_height = 44.0;
                    if is_resizing {
                        let max_width = (image_width - start_bounds.x).max(min_width as i32) as f64;
                        let max_height =
                            (image_height - start_bounds.y).max(min_height as i32) as f64;
                        bounds.rect.x = start_bounds.x;
                        bounds.rect.y = start_bounds.y;
                        bounds.rect.width = ((start_bounds.width as f64 + dx)
                            .clamp(min_width, max_width))
                        .round() as i32;
                        bounds.rect.height = ((start_bounds.height as f64 + dy)
                            .clamp(min_height, max_height))
                        .round() as i32;
                    } else {
                        match handle {
                            Some(MoveHandle::Left) => {
                                // Mirror the Right handle exactly:
                                // right edge is fixed, x moves with dx, width = right - x.
                                let right = start_bounds.x + start_bounds.width;
                                let proposed_x = start_bounds.x + dx.round() as i32;
                                // x can't go below 0 or past (right - min_width)
                                let new_x = proposed_x.clamp(0, (right - min_width as i32).max(0));
                                bounds.rect.x = new_x;
                                bounds.rect.width = (right - new_x).max(min_width as i32);
                                bounds.rect.y = start_bounds.y;
                                bounds.rect.height = start_bounds.height;
                            }
                            Some(MoveHandle::Right) => {
                                let max_width =
                                    (image_width - start_bounds.x).max(min_width as i32) as f64;
                                bounds.rect.x = start_bounds.x;
                                bounds.rect.y = start_bounds.y;
                                bounds.rect.height = start_bounds.height;
                                bounds.rect.width = ((start_bounds.width as f64 + dx)
                                    .clamp(min_width, max_width))
                                .round() as i32;
                            }
                            None => {}
                        }
                    }
                    bounds.rect.x = bounds
                        .rect
                        .x
                        .clamp(0, (image_width - bounds.rect.width).max(0));
                    bounds.rect.y = bounds
                        .rect
                        .y
                        .clamp(0, (image_height - bounds.rect.height).max(0));
                    bounds.sync_handles();
                }
                if st.active_text_input.is_some() {
                    if is_resizing {
                        st.fit_active_text_to_layout_preserving_box();
                    } else {
                        st.fit_active_text_height_only();
                    }
                } else if !is_resizing {
                    // Committed action circle-handle resize: reflow height so
                    // text never overflows the bottom of the box.
                    st.fit_committed_text_height_only();
                }
                // Keep the original drag anchor fixed while using drag-start bounds.
            }

            if let Some(area) = drawing_area_motion.upgrade() {
                area.queue_draw();
            }
        }
    });

    let eyedropper_mode_motion_leave = eyedropper_mode.clone();
    let eyedropper_point_motion_leave = eyedropper_point.clone();
    let canvas_eyedropper_ring_motion_leave = canvas_eyedropper_ring.clone();
    let window_motion_leave = window.downgrade();
    motion.connect_leave(move |_| {
        *eyedropper_point_motion_leave.borrow_mut() = None;
        canvas_eyedropper_ring_motion_leave.set_visible(false);

        if let Some(window) = window_motion_leave.upgrade() {
            if eyedropper_mode_motion_leave.get() {
                set_window_cursor_name(&window, Some("crosshair"));
            } else {
                set_window_cursor_name(&window, None);
            }
        }
    });

    drawing_area.add_controller(motion);

    let key_controller = EventControllerKey::new();
    let state_keys = state.clone();
    let drawing_area_keys = drawing_area.downgrade();
    let tool_buttons_keys = tool_buttons.clone();
    let apply_crop_btn_keys = apply_crop_btn.clone();
    let update_toolbar_for_tool_keys = update_toolbar_for_tool.clone();
    let update_crop_size_fields_keys = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_keys = sync_picker_for_active_tool.clone();
    let sync_select_inspector_keys = sync_select_inspector.clone();
    let eyedropper_mode_keys = eyedropper_mode.clone();
    let eyedropper_point_keys = eyedropper_point.clone();
    let eyedropper_rendered_keys = eyedropper_rendered.clone();
    let canvas_eyedropper_ring_keys = canvas_eyedropper_ring.clone();
    let window_keys = window.downgrade();

    let zoom_level_keys = zoom_level.clone();
    let update_canvas_content_size_keys = update_canvas_content_size.clone();
    let zoom_popup_keys = zoom_popup.clone();

    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gdk::Key::Escape && eyedropper_mode_keys.get() {
            eyedropper_mode_keys.set(false);
            *eyedropper_point_keys.borrow_mut() = None;
            *eyedropper_rendered_keys.borrow_mut() = None;
            canvas_eyedropper_ring_keys.set_visible(false);
            if let Some(window) = window_keys.upgrade() {
                set_window_cursor_name(&window, None);
            }
            return glib::Propagation::Stop;
        }

        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        let pressed = key.to_unicode();

        {
            let mut st = state_keys.lock().unwrap();
            if st.active_text_input.is_some() {
                let mut should_cancel = false;
                let mut handled = true;

                match key {
                    gdk::Key::Escape => should_cancel = true,
                    gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\n'),
                    gdk::Key::BackSpace => st.delete_text_input_char(),
                    gdk::Key::space => st.add_text_input_char(' '),
                    gdk::Key::Left => st.move_cursor_left(),
                    gdk::Key::Right => st.move_cursor_right(),
                    _ => {
                        if !ctrl {
                            if let Some(ch) = pressed {
                                if !ch.is_control() {
                                    st.add_text_input_char(ch);
                                } else {
                                    handled = false;
                                }
                            } else {
                                handled = false;
                            }
                        } else {
                            handled = false;
                        }
                    }
                }

                if should_cancel {
                    st.cancel_text_input();
                }

                if handled && st.active_text_input.is_some() {
                    st.fit_active_text_to_layout();
                    st.reset_text_cursor_blink();
                }

                if handled || should_cancel {
                    drop(st);
                    if let Some(area) = drawing_area_keys.upgrade() {
                        area.queue_draw();
                    }
                    return glib::Propagation::Stop;
                }
            }
        }

        if ctrl && (pressed == Some('z') || pressed == Some('Z')) {
            let changed = if shift {
                state_keys.lock().unwrap().redo()
            } else {
                state_keys.lock().unwrap().undo()
            };
            if changed {
                sync_select_inspector_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if ctrl && (pressed == Some('y') || pressed == Some('Y')) {
            if state_keys.lock().unwrap().redo() {
                sync_select_inspector_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if ctrl {
            let mut handled = false;
            match key {
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                    zoom_level_keys.set(clamp_zoom_level(zoom_level_keys.get() * ZOOM_STEP));
                    handled = true;
                }
                gdk::Key::minus | gdk::Key::underscore | gdk::Key::KP_Subtract => {
                    zoom_level_keys.set(clamp_zoom_level(zoom_level_keys.get() / ZOOM_STEP));
                    handled = true;
                }
                gdk::Key::_0 | gdk::Key::KP_0 => {
                    zoom_level_keys.set(1.0);
                    handled = true;
                }
                gdk::Key::_2 | gdk::Key::KP_2 => {
                    zoom_level_keys.set(clamp_zoom_level(1.5));
                    handled = true;
                }
                _ => {}
            }

            if handled {
                update_canvas_content_size_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
                zoom_popup_keys.set_visible(false);
                return glib::Propagation::Stop;
            }
        }

        if !ctrl {
            if let Some((tool, active_button)) = pressed.and_then(tool_shortcut_target) {
                set_active_tool_button(&tool_buttons_keys, active_button);
                let has_crop_selection = {
                    let mut st = state_keys.lock().unwrap();
                    st.set_tool(tool);
                    if matches!(tool, Tool::Crop) {
                        st.ensure_crop_selection_initialized();
                    }
                    st.crop_selection.is_some()
                };
                update_toolbar_for_tool_keys(tool);
                sync_select_inspector_keys();
                sync_picker_for_active_tool_keys();
                set_crop_apply_button_state(
                    &apply_crop_btn_keys,
                    matches!(tool, Tool::Crop),
                    has_crop_selection,
                );
                update_crop_size_fields_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }
        }

        if (key == gdk::Key::Delete || key == gdk::Key::BackSpace)
            && state_keys.lock().unwrap().remove_selected_action()
        {
            sync_select_inspector_keys();
            if let Some(area) = drawing_area_keys.upgrade() {
                area.queue_draw();
            }
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_context_uses_zoom_footer_fields_instead_of_pin_state() {
        let source = include_str!("events.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("pub zoom_button: Button,")
                && production_source.contains("pub zoom_label: Label,")
                && production_source.contains("pub zoom_popup: GtkBox,")
                && production_source.contains("pub zoom_in_btn: Button,")
                && production_source.contains("pub zoom_out_btn: Button,")
                && production_source.contains("pub fit_to_screen_btn: Button,")
                && !production_source.contains("pub pin_btn: Button,")
                && !production_source.contains("pub initial_pin_state: bool,"),
            "EventContext should be updated to drive footer zoom controls instead of pinning state",
        );
    }

    #[test]
    fn footer_zoom_actions_update_transform_and_label() {
        let source = include_str!("events.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("zoom_button.connect_clicked(move |_| {")
                && production_source.contains("let becoming_visible = !zoom_popup_btn.is_visible();")
                && production_source.contains("zoom_popup_btn.set_visible(becoming_visible);")
                && production_source.contains("zoom_in_btn.connect_clicked(move |b| {")
                && production_source.contains("zoom_out_btn.connect_clicked(move |b| {")
                && production_source.contains("fit_to_screen_btn.connect_clicked(move |b| {")
                && production_source.contains("zoom_popup_in.set_visible(false);")
                && production_source.contains("update_canvas_content_size();")
                && production_source.contains("drawing_area.queue_draw();")
                && production_source.contains("scroll_controller.connect_scroll(move |_, _dx, dy| {"),
            "Footer zoom actions should open the popover, update the zoom state, refresh canvas layout, and support wheel zoom",
        );
    }

    #[test]
    fn enter_key_inserts_newline_in_text_input() {
        let source = include_str!("events.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("if keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter {")
                && production_source.contains("gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\\n'),"),
            "Enter should insert a newline character in the text input, not commit or be cancelled by the legacy text-bounds handler",
        );
    }
}
