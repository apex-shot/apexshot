use gtk4::{
    glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    DrawingArea, Label, Overlay, Popover, Scale, ScrolledWindow,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::{
    state::EditorState,
    types::{DrawColor, Tool, ViewTransform},
};

/// Shared hit radii for click/motion/drag handle testing.
pub(super) const MOVE_HANDLE_DRAG_RADIUS: f64 = 10.0;
pub(super) const RESIZE_HANDLE_DRAG_SIZE: f64 = 18.0;

mod click;
mod crop;
mod drag;
mod history;
mod interaction;
mod keyboard;
mod motion;
mod options;
mod output;
mod tools;
mod zoom;

use click::wire_canvas_click;
use crop::wire_crop_action_buttons;
use drag::wire_canvas_drag;
use history::wire_history_buttons;
use interaction::SpacePanState;
use keyboard::wire_window_keyboard;
use motion::wire_canvas_motion;
use options::{wire_tool_options, ToolOptionsParts};
use output::wire_output_lifecycle;
pub(super) use output::persist_image_session;
use tools::{wire_tool_mode_switches, ToolModeButtons};
use zoom::wire_zoom_controls;

// Re-export for setup (`window/mod.rs`) which constructs the bundle before wiring.
pub(super) use interaction::EyedropperBundle;

pub(super) struct EventContext {
    pub app: Application,
    pub window: ApplicationWindow,
    pub path: PathBuf,
    /// Set to false when this editor session is superseded (e.g. the empty
    /// editor window is reused to load a real image). Stale window-level
    /// signal handlers must become no-ops.
    pub session_alive: Rc<Cell<bool>>,
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
    pub eyedropper: EyedropperBundle,
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
        session_alive,
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
        eyedropper,
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

    let space_pan = SpacePanState::new();
    let space_pan_active = space_pan.active.clone();
    let space_pan_dragging = space_pan.dragging.clone();
    let space_pan_origin = space_pan.origin.clone();

    let eyedropper_mode = eyedropper.mode.clone();
    let eyedropper_from_sidebar = eyedropper.from_sidebar.clone();
    let eyedropper_point = eyedropper.point.clone();
    let eyedropper_rendered = eyedropper.rendered.clone();
    let canvas_eyedropper_ring = eyedropper.ring.clone();

    let window_minimize = window.downgrade();
    traffic_minimize.connect_clicked(move |_| {
        if let Some(window) = window_minimize.upgrade() {
            window.minimize();
        }
    });

    let window_zoom = window.downgrade();
    traffic_zoom.connect_clicked(move |_| {
        if let Some(window) = window_zoom.upgrade() {
            window.set_fullscreened(!window.is_fullscreen());
        }
    });

    let apply_zoom_change = wire_zoom_controls(
        &window,
        &state,
        &transform,
        &drawing_area,
        &canvas_scroller,
        &zoom_button,
        &zoom_header_label,
        &zoom_popup,
        &zoom_minus_btn,
        &zoom_plus_btn,
        &zoom_in_btn,
        &zoom_out_btn,
        &fit_to_screen_btn,
        &zoom_to_selection_btn,
        &zoom_level,
        &update_canvas_content_size,
    );

    wire_output_lifecycle(
        &app,
        &window,
        &path,
        &state,
        &copy_btn,
        &upload_btn,
        &save_btn,
        &traffic_close,
    );

    // Tool-mode activation (Select/Crop/Background/Pen/.../Focus). Distinct toggle
    // policies stay in tools.rs and must not be collapsed into one handler.
    wire_tool_mode_switches(
        ToolModeButtons {
            tool_buttons: &tool_buttons,
            select: &select_btn,
            crop: &crop_btn,
            background: &background_btn,
            pen: &draw_btn,
            arrow: &arrow_btn,
            line: &line_btn,
            boxed: &box_btn,
            circle: &circle_btn,
            text: &text_btn,
            number: &number_btn,
            highlighter: &highlighter_btn,
            obfuscate: &obfuscate_btn,
            focus: &focus_btn,
            apply_crop: &apply_crop_btn,
        },
        &window,
        &state,
        &drawing_area,
        &update_toolbar_for_tool,
        &update_crop_size_fields,
        &sync_picker_for_active_tool,
        &sync_select_inspector,
        &sync_size_control,
        &rebuild_effects_async,
    );

    // Crop apply/reset inspector actions (not canvas crop drag).
    wire_crop_action_buttons(
        &apply_crop_btn,
        &crop_reset_btn,
        &state,
        &drawing_area,
        &update_canvas_content_size,
        &update_crop_size_fields,
    );

    // Inspector/toolbar tool options (weight, style, numbering, palette, size).
    wire_tool_options(
        ToolOptionsParts {
            pen_weight_button: &pen_weight_button,
            pen_weight_list: &pen_weight_list,
            highlighter_weight_list: &highlighter_weight_list,
            obfuscate_method_button: &obfuscate_method_button,
            obfuscate_method_list: &obfuscate_method_list,
            arrow_style_button: &arrow_style_button,
            arrow_style_list: &arrow_style_list,
            arrow_thickness_list: &arrow_thickness_list,
            stroke_size_button: &stroke_size_button,
            stroke_size_list: &stroke_size_list,
            inverse_direction_toggle: &inverse_direction_toggle,
            number_options_list: &number_options_list,
            number_start_entry: &number_start_entry,
            number_inc_btn: &number_inc_btn,
            number_dec_btn: &number_dec_btn,
            number_size_button: &number_size_button,
            number_size_list: &number_size_list,
            color_buttons: &color_buttons,
            color_picker_dot: &color_picker_dot,
            color_class_names: &color_class_names,
            color_popover: &color_popover,
            size_slider: &size_slider,
        },
        &window,
        &state,
        &drawing_area,
        &sync_picker_from_color,
        &sync_picker_for_active_tool,
        &sync_size_control,
        &rebuild_effects_async,
    );

    wire_history_buttons(
        &undo_btn,
        &redo_btn,
        &delete_selected_btn,
        &state,
        &drawing_area,
        &rebuild_effects_async,
        &sync_size_control,
        &sync_select_inspector,
    );

    wire_canvas_drag(
        &window,
        &state,
        &transform,
        &drawing_area,
        &canvas_scroller,
        &apply_crop_btn,
        &space_pan_active,
        &space_pan_dragging,
        &space_pan_origin,
        &eyedropper_mode,
        &update_crop_size_fields,
        &rebuild_effects_async,
        &sync_size_control,
        &sync_select_inspector,
    );

    wire_canvas_click(
        &window,
        &state,
        &transform,
        &drawing_area,
        &color_buttons,
        &color_picker_dot,
        &color_class_names,
        &color_popover,
        &space_pan_active,
        &eyedropper_mode,
        &eyedropper_from_sidebar,
        &eyedropper_point,
        &eyedropper_rendered,
        &canvas_eyedropper_ring,
        &set_picker_panel_visibility,
        &apply_picker_color_to_editor,
        &sync_picker_from_color,
        &add_color_to_custom_slots,
        &sync_size_control,
        &text_size_label,
        &font_family_label,
        &text_size_list,
        &font_family_list,
        &sync_select_inspector,
    );

    wire_canvas_motion(
        &window,
        &state,
        &transform,
        &drawing_area,
        &space_pan_active,
        &space_pan_dragging,
        &eyedropper_mode,
        &eyedropper_point,
        &canvas_eyedropper_ring,
    );

    wire_window_keyboard(
        &window,
        &state,
        &drawing_area,
        &tool_buttons,
        &apply_crop_btn,
        &space_pan_active,
        &space_pan_dragging,
        &eyedropper_mode,
        &eyedropper_point,
        &eyedropper_rendered,
        &canvas_eyedropper_ring,
        &zoom_level,
        &apply_zoom_change,
        &zoom_popup,
        &update_toolbar_for_tool,
        &update_crop_size_fields,
        &sync_picker_for_active_tool,
        &sync_select_inspector,
    );

    let app_weak = app.downgrade();
    let session_alive_close = session_alive.clone();
    window.connect_close_request(move |_| {
        // A stale handler from a superseded session must not quit the app.
        if !session_alive_close.get() {
            return glib::Propagation::Proceed;
        }
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });
}

#[cfg(test)]
mod tests {
    fn production_events_source() -> String {
        let mod_src = include_str!("mod.rs");
        let zoom_src = include_str!("zoom.rs");
        let history_src = include_str!("history.rs");
        let output_src = include_str!("output.rs");
        let tools_src = include_str!("tools.rs");
        let crop_src = include_str!("crop.rs");
        let options_src = include_str!("options.rs");
        let interaction_src = include_str!("interaction.rs");
        let drag_src = include_str!("drag.rs");
        let click_src = include_str!("click.rs");
        let motion_src = include_str!("motion.rs");
        let keyboard_src = include_str!("keyboard.rs");
        let mod_prod = mod_src.split("#[cfg(test)]").next().unwrap_or(mod_src);
        let output_prod = output_src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(output_src);
        let tools_prod = tools_src.split("#[cfg(test)]").next().unwrap_or(tools_src);
        let crop_prod = crop_src.split("#[cfg(test)]").next().unwrap_or(crop_src);
        let options_prod = options_src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(options_src);
        let interaction_prod = interaction_src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(interaction_src);
        let drag_prod = drag_src.split("#[cfg(test)]").next().unwrap_or(drag_src);
        let click_prod = click_src.split("#[cfg(test)]").next().unwrap_or(click_src);
        let motion_prod = motion_src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(motion_src);
        let keyboard_prod = keyboard_src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(keyboard_src);
        format!(
            "{mod_prod}\n{zoom_src}\n{history_src}\n{output_prod}\n{tools_prod}\n{crop_prod}\n{options_prod}\n{interaction_prod}\n{drag_prod}\n{click_prod}\n{motion_prod}\n{keyboard_prod}"
        )
    }

    #[test]
    fn event_context_uses_zoom_footer_fields_instead_of_pin_state() {
        let production_source = production_events_source();
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
        let production_source = production_events_source();
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
                && production_source.contains("scroll_controller.connect_scroll(move |controller, dx, dy| {")
                && production_source.contains("let delta = if dy != 0.0 { dy } else { dx };")
                && production_source.contains("gdk::ModifierType::CONTROL_MASK"),
            "Footer zoom actions should open the popover, update the zoom state, refresh canvas layout, and support Ctrl-wheel zoom",
        );
    }

    #[test]
    fn canvas_scrolls_normally_and_space_enables_primary_button_panning() {
        let production_source = production_events_source();
        assert!(
            production_source.contains("return glib::Propagation::Proceed;")
                && production_source.contains("if space_pan_active_drag_begin.get() {")
                && production_source.contains("space_pan_dragging_begin.set(true);")
                && production_source.contains("space_pan_controller.set_propagation_phase(gtk4::PropagationPhase::Capture)")
                && production_source.contains("if key != gdk::Key::space || eyedropper_mode_space.get()")
                && production_source.contains("Some(\"grab\")"),
            "normal scrolling should reach the canvas scroller while capture-phase space enables hand panning"
        );
    }

    #[test]
    fn enter_key_inserts_newline_in_text_input() {
        let production_source = production_events_source();
        assert!(
            !production_source.contains("if keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter {")
                && production_source.contains("gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\\n'),"),
            "Enter should insert a newline character in the text input, not commit or be cancelled by the legacy text-bounds handler",
        );
    }

    #[test]
    fn interaction_state_bundles_and_zoom_callback_are_wired() {
        let production_source = production_events_source();
        assert!(
            production_source.contains("let space_pan = SpacePanState::new();")
                && production_source.contains("pub eyedropper: EyedropperBundle,")
                && production_source.contains("let apply_zoom_change = wire_zoom_controls(")
                && production_source.contains("apply_zoom_change_keys(zoom_level_keys.get() * ZOOM_STEP);")
                && production_source.contains("gdk::Key::_2 | gdk::Key::KP_2 => {")
                && production_source.contains("apply_zoom_change_keys(1.5);")
                && !production_source.contains("zoom_level_keys.set(clamp_zoom_level(1.5));"),
            "PR 10.15 must own SpacePanState/EyedropperBundle and reuse wire_zoom_controls callback including Ctrl+2 → 1.5"
        );
    }

    #[test]
    fn canvas_drag_family_is_wired_from_facade() {
        let production_source = production_events_source();
        let mod_prod = include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(
            mod_prod.contains("wire_canvas_drag(")
                && !mod_prod.contains("let drag = GestureDrag::new();")
                && !mod_prod.contains("drag.connect_drag_begin(")
                && production_source.contains("pub(super) fn wire_canvas_drag(")
                && production_source.contains("drag.connect_drag_begin(move |gesture, x, y| {")
                && production_source.contains("drag.connect_drag_update(move |gesture, offset_x, offset_y| {")
                && production_source.contains("drag.connect_drag_end(move |gesture, offset_x, offset_y| {")
                && production_source.contains("drawing_area.add_controller(drag);"),
            "PR 10.16 must call wire_canvas_drag from the facade; GestureDrag body lives in drag.rs"
        );
    }

    #[test]
    fn canvas_click_family_is_wired_from_facade() {
        let production_source = production_events_source();
        let mod_prod = include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(
            mod_prod.contains("wire_canvas_click(")
                && !mod_prod.contains("let click = GestureClick::new();")
                && !mod_prod.contains("click.connect_pressed(")
                && !mod_prod.contains("fn sync_text_option_selection")
                && !mod_prod.contains("const TEXT_SIZE_OPTIONS")
                && production_source.contains("pub(super) fn wire_canvas_click(")
                && production_source.contains("click.connect_pressed(move |gesture, n_press, x, y| {")
                && production_source.contains("click.connect_released(move |_gesture, _n_press, _x, _y| {")
                && production_source.contains("drawing_area.add_controller(click);")
                && production_source.contains("cancel_text_edit()"),
            "PR 10.17 must call wire_canvas_click from the facade; click/Escape/text-sync live in click.rs"
        );
    }

    #[test]
    fn canvas_motion_and_window_keyboard_are_wired_from_facade() {
        let production_source = production_events_source();
        let mod_prod = include_str!("mod.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or("");
        assert!(
            mod_prod.contains("wire_canvas_motion(")
                && mod_prod.contains("wire_window_keyboard(")
                && !mod_prod.contains("let motion = EventControllerMotion::new();")
                && !mod_prod.contains("motion.connect_motion(")
                && !mod_prod.contains("let space_pan_controller = EventControllerKey::new();")
                && !mod_prod.contains("space_pan_controller.set_propagation_phase(")
                && production_source.contains("pub(super) fn wire_canvas_motion(")
                && production_source.contains("pub(super) fn wire_window_keyboard(")
                && production_source.contains("motion.connect_motion(move |_, x, y| {")
                && production_source.contains("space_pan_controller.set_propagation_phase(gtk4::PropagationPhase::Capture)")
                && production_source.contains("apply_zoom_change_keys(1.5);"),
            "PR 10.18 must call wire_canvas_motion and wire_window_keyboard; bodies live in motion.rs/keyboard.rs"
        );
    }
}
