use gdk4x11::X11Surface;
use gtk4::gdk;
use gtk4::{
    glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    DrawingArea, Entry, Label, Orientation, Overlay, Popover, Stack,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use x11rb::{connection::Connection, protocol::xproto, protocol::xproto::ConnectionExt};

use self::background_panel::BACKGROUND_SIDEBAR_WIDTH;
use super::color::{draw_color_to_hex, draw_color_to_rgba_u8, selection_hit_padding_for_scale};
use super::pen_weight::PenWeight;
use super::render::{
    draw_active_text_input, draw_annotation_action, draw_arrow_control_handles,
    draw_arrow_selection_outline, draw_canvas_checkerboard_background, draw_crop_overlay,
    draw_draft_action, draw_focus_overlay, draw_rgba_to_context, draw_selection_handles,
    draw_selection_outline, draw_text_edit_border, draw_text_edit_handles, rgba_image_to_surface,
    text_action_bounds,
};
use super::selection::{action_bounds_with_padding, action_resize_handles};
use super::state::{apply_effect_actions, EditorState};
use super::types::{
    AnnotationAction, ArrowStyle, BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor,
    EditorError, Point, Rect, Tool, ViewTransform,
};

#[derive(Debug, Clone, Copy)]
pub struct AnnotateRuntimeConfig {
    pub inverse_arrow_direction: bool,
    pub smooth_drawing: bool,
    pub draw_object_shadow: bool,
    pub auto_expand_canvas: bool,
    pub show_color_names: bool,
    pub always_on_top: bool,
    pub show_dock_icon: bool,
}

impl AnnotateRuntimeConfig {
    pub fn from_app_config(config: &crate::config::AppConfig) -> Self {
        Self {
            inverse_arrow_direction: config.annotate_inverse_arrow,
            smooth_drawing: config.annotate_smooth_drawing,
            draw_object_shadow: config.annotate_draw_shadow,
            auto_expand_canvas: config.annotate_auto_expand,
            show_color_names: config.annotate_show_color_names,
            always_on_top: config.annotate_always_on_top,
            show_dock_icon: config.annotate_show_dock_icon,
        }
    }
}

fn build_arrow_thickness_preview(weight: super::pen_weight::PenWeight) -> DrawingArea {
    let preview = DrawingArea::new();
    preview.set_content_width(22);
    preview.set_content_height(16);
    preview.set_draw_func(move |_, context, width, height| {
        let stroke_width = match weight {
            PenWeight::Small => 2.0,
            PenWeight::Medium => 4.0,
            PenWeight::Large => 7.0,
            PenWeight::ExtraLarge => 10.0,
        };
        context.set_source_rgba(241.0 / 255.0, 241.0 / 255.0, 243.0 / 255.0, 0.92);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        context.set_line_width(stroke_width);
        let center_y = f64::from(height) / 2.0;
        context.move_to(3.0, center_y);
        context.line_to(f64::from(width) - 3.0, center_y);
        let _ = context.stroke();
    });
    preview
}

fn stroke_size_option_index(stroke_size: f64) -> usize {
    match stroke_size.round() as i32 {
        2 => 0,
        4 => 1,
        7 => 2,
        12 => 3,
        _ => 1,
    }
}

fn pen_weight_option_index(stroke_size: f64) -> usize {
    match stroke_size.round() as i32 {
        8 => 0,
        16 => 1,
        24 => 2,
        32 => 3,
        _ => 1,
    }
}

use super::ui_support::{
    arrow_style_toolbar_icon, install_edge_resize, install_editor_css, install_window_drag,
    prefers_dark_glass_theme, prefers_reduced_transparency,
    recommended_window_size_with_extra_width, tool_icon_widget, toolbar_icon_size,
};

const TEXT_SIZE_OPTIONS: [i32; 12] = [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72];
const TEXT_FONT_FAMILIES: [&str; 5] = ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"];
const OBFUSCATE_METHOD_OPTIONS: [(super::types::ObfuscateMethod, &str); 4] = [
    (super::types::ObfuscateMethod::Pixelate, "Pixelate"),
    (super::types::ObfuscateMethod::BlurSecure, "Blur (Secure)"),
    (super::types::ObfuscateMethod::BlurSmooth, "Blur (Smooth)"),
    (super::types::ObfuscateMethod::Blackout, "Blackout"),
];

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

fn sync_crop_option_selection(list: &GtkBox, selected_index: usize) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();
        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        if index == selected_index {
            button.add_css_class("editor-crop-inspector-option-active");
        } else {
            button.remove_css_class("editor-crop-inspector-option-active");
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

fn sync_obfuscate_option_selection(list: &GtkBox, selected_index: usize) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();
        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        if index == selected_index {
            button.add_css_class("editor-obfuscate-inspector-option-active");
        } else {
            button.remove_css_class("editor-obfuscate-inspector-option-active");
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

pub mod background_panel;
mod canvas;
pub mod color_picker;
pub mod colors_panel;
#[allow(dead_code)]
mod cursor;
mod events;
mod footer;
mod toolbar;

#[allow(dead_code)]
pub mod icon_names {
    pub use shipped::*;
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}

pub fn open_image_editor(path: PathBuf) -> Result<(), EditorError> {
    if !path.exists() {
        return Err(EditorError::MissingFile(path));
    }

    let app = Application::builder()
        .application_id("com.apexshot.capture.editor")
        .build();

    app.connect_activate(move |application| {
        setup_editor_window(application, path.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

#[allow(unused_imports)]
pub(super) use cursor::cursor_name_for_view_point;

fn env_var_contains_case_insensitive(key: &str, needle: &str) -> bool {
    std::env::var(key)
        .map(|value| value.to_lowercase().contains(&needle.to_lowercase()))
        .unwrap_or(false)
}

fn is_gnome_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && (env_var_contains_case_insensitive("XDG_CURRENT_DESKTOP", "gnome")
            || env_var_contains_case_insensitive("DESKTOP_SESSION", "gnome"))
}

fn next_tracked_window_id(role: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{role}-{stamp}")
}

fn intern_atom<C: Connection>(conn: &C, atom_name: &[u8]) -> Result<u32, String> {
    conn.intern_atom(false, atom_name)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())
        .map(|reply| reply.atom)
}

fn send_net_wm_state_client_message<C: Connection>(
    conn: &C,
    root: xproto::Window,
    window: xproto::Window,
    net_wm_state_atom: u32,
    action: u32,
    first_property: u32,
    second_property: u32,
) -> Result<(), String> {
    let client_message = xproto::ClientMessageEvent::new(
        32,
        window,
        net_wm_state_atom,
        [action, first_property, second_property, 1, 0],
    );

    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        client_message,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn request_x11_state(
    window: &ApplicationWindow,
    state_atom_name: &[u8],
    enable: bool,
) -> Result<(), String> {
    let surface = window
        .surface()
        .ok_or_else(|| "missing GTK surface".to_string())?;
    let x11_surface = surface
        .downcast::<X11Surface>()
        .map_err(|_| "surface is not X11".to_string())?;

    let xid = u32::try_from(x11_surface.xid())
        .map_err(|_| "X11 window id is out of range".to_string())?;
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .map(|screen| screen.root)
        .ok_or_else(|| "missing X11 root window".to_string())?;

    let net_wm_state = intern_atom(&conn, b"_NET_WM_STATE")?;
    let state_atom = intern_atom(&conn, state_atom_name)?;
    let action = if enable { 1 } else { 0 };
    send_net_wm_state_client_message(&conn, root, xid, net_wm_state, action, state_atom, 0)?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn set_window_always_on_top(
    window: &ApplicationWindow,
    tracked_id: &str,
    enabled: bool,
    title: &str,
    namespace: &str,
) {
    if is_gnome_wayland_session() {
        if enabled {
            crate::gnome_integration::emit_tracked_window_opened(
                tracked_id,
                std::process::id(),
                title,
                "annotate-editor",
                namespace,
            );
        } else {
            crate::gnome_integration::emit_tracked_window_closed(tracked_id);
        }
    }

    let _ = request_x11_state(window, b"_NET_WM_STATE_ABOVE", enabled);
    let _ = request_x11_state(window, b"_NET_WM_STATE_STICKY", enabled);
}

fn set_window_dock_visibility(window: &ApplicationWindow, show_dock_icon: bool) {
    let hide = !show_dock_icon;
    let _ = request_x11_state(window, b"_NET_WM_STATE_SKIP_TASKBAR", hide);
    let _ = request_x11_state(window, b"_NET_WM_STATE_SKIP_PAGER", hide);
}

pub fn setup_editor_window(app: &Application, path: PathBuf) {
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    install_editor_css();
    let annotate_config =
        AnnotateRuntimeConfig::from_app_config(&crate::config::load_config().sanitized());

    let drawing_area_placeholder = Rc::new(RefCell::new(
        None::<glib::object::WeakRef<gtk4::DrawingArea>>,
    ));

    let image = match image::open(&path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("Failed to load image for editing: {e}");
            app.quit();
            return;
        }
    };

    let (img_width, img_height) = image.dimensions();
    let state = Arc::new(Mutex::new(EditorState::new(image.clone())));
    {
        let mut st = state.lock().unwrap();
        st.inverse_arrow_direction = annotate_config.inverse_arrow_direction;
        st.smooth_drawing_enabled = annotate_config.smooth_drawing;
        st.draw_object_shadow = annotate_config.draw_object_shadow;
        st.auto_expand_canvas = annotate_config.auto_expand_canvas;
        let detector = st.text_detector.clone();
        let ready_flag = st.text_detection_ready.clone();
        st.text_detection_handle = Some(super::text_detect::spawn_text_detection(
            image, detector, ready_flag,
        ));
    }
    let transform = Arc::new(Mutex::new(ViewTransform::for_image(
        img_width as f64,
        img_height as f64,
    )));
    let zoom_level = Rc::new(Cell::new(1.0_f64));

    let (default_width, default_height) =
        recommended_window_size_with_extra_width(img_width as i32, img_height as i32, 280);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Screenshot Editor")
        .default_width(default_width)
        .default_height(default_height)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");

    let _dark_glass = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();
    root.add_css_class("editor-theme-dark");
    if reduced_transparency {
        root.add_css_class("editor-reduced-transparency");
    }

    let toolbar::ToolbarBaseParts {
        root: toolbar,
        traffic_close,
        traffic_minimize,
        traffic_zoom,
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
        sep_1,
        sep_2,
    } = toolbar::build_toolbar_base(toolbar::ToolbarBaseIconNames {
        crop: icon_names::CROP,
        draw: icon_names::PEN_REGULAR,
        arrow: icon_names::ARROW_UP_RIGHT_REGULAR,
        line: icon_names::DRAW_LINE,
        box_: icon_names::RECTANGLE_LANDSCAPE_REGULAR,
        circle: icon_names::CIRCLE_REGULAR,
        text: icon_names::TEXT_T_REGULAR,
        number: icon_names::NUMBER_CIRCLE_1_REGULAR,
        highlighter: icon_names::HIGHLIGHT_REGULAR,
        obfuscate: icon_names::FOG,
        focus: icon_names::SMALL_RECTANGLE_IN_FOCUS,
        obfuscate_pixelate: icon_names::VIEW_GRID,
        obfuscate_blur_secure: icon_names::SHIELD_REGULAR,
        obfuscate_blur_smooth: icon_names::BLUR,
        obfuscate_blackout: icon_names::MEDIA_PLAYBACK_STOP,
    });

    let canvas_queue_draw_signal: Rc<dyn Fn()> = Rc::new({
        let drawing_area_placeholder = drawing_area_placeholder.clone();
        move || {
            if let Some(weak) = drawing_area_placeholder.borrow().as_ref() {
                if let Some(area) = weak.upgrade() {
                    area.queue_draw();
                }
            }
        }
    });

    let color_picker_parts = color_picker::build_color_picker(
        state.clone(),
        canvas_queue_draw_signal,
        drawing_area_placeholder.clone(),
        annotate_config.show_color_names,
    );
    let _color_picker_trigger_host = color_picker_parts.trigger_host;
    let color_popover = color_picker_parts.popover;
    let color_buttons = color_picker_parts.color_buttons;
    let color_picker_dot = color_picker_parts.color_picker_dot;
    let color_class_names = color_picker_parts.color_class_names;
    let eyedropper_btn = color_picker_parts.eyedropper_btn;
    let sync_picker_for_active_tool = color_picker_parts.sync_for_active_tool;
    let sync_picker_from_color = color_picker_parts.sync_picker_from_color;
    let apply_picker_color_to_editor = color_picker_parts.apply_picker_color;
    let set_picker_panel_visibility = color_picker_parts.set_picker_panel_visibility;
    let custom_slot_colors = color_picker_parts.custom_slot_colors;
    let refresh_custom_color_slots = color_picker_parts.refresh_custom_color_slots;
    let register_color_panel_sync = color_picker_parts.register_external_sync;
    let sidebar_eyedropper_activation = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));

    let toolbar::ToolbarModeParts {
        root: center_group,
        toolbar_mode_stack,
        color_status,
        color_status_swatch,
        color_status_label,
        size_group,
        size_slider,
        text_size_group,
        text_size_label,
        text_size_list: _toolbar_text_size_list,
        font_family_group,
        font_family_label,
        font_family_list: _toolbar_font_family_list,
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover: _,
        obfuscate_method_list: _toolbar_obfuscate_method_list,
        pen_weight_button,
        pen_weight_popover: _,
        pen_weight_list: _toolbar_pen_weight_list,
        pen_weight_group,
        number_options_popover: _,
        number_options_list: _toolbar_number_options_list,
        number_start_entry: _toolbar_number_start_entry,
        number_inc_btn: _toolbar_number_inc_btn,
        number_dec_btn: _toolbar_number_dec_btn,
        number_size_button,
        number_size_popover: _,
        number_size_list: _toolbar_number_size_list,
        number_options_group,
        arrow_style_group,
        arrow_style_button,
        arrow_style_popover: _,
        arrow_style_list: _toolbar_arrow_style_list,
        stroke_size_group,
        stroke_size_button,
        stroke_size_popover: _,
        stroke_size_list: _toolbar_stroke_size_list,
    } = toolbar::build_toolbar_mode_controls(
        &crop_btn,
        &background_btn,
        &select_btn,
        &draw_btn,
        &box_btn,
        &circle_btn,
        &arrow_btn,
        &line_btn,
        &text_btn,
        icon_names::TEXT_ITALIC_REGULAR,
        &obfuscate_btn,
        &focus_btn,
        &number_btn,
        &highlighter_btn,
        &sep_1,
        &sep_2,
    );
    toolbar.set_center_widget(Some(&center_group));

    let toolbar_right_parts = toolbar::build_toolbar_right_controls(
        &color_status,
        icon_names::ARROW_UNDO_REGULAR,
        icon_names::ARROW_REDO_REGULAR,
        icon_names::DELETE_REGULAR,
    );
    let undo_btn = toolbar_right_parts.undo_btn;
    let redo_btn = toolbar_right_parts.redo_btn;
    let delete_selected_btn = toolbar_right_parts.delete_selected_btn;
    let save_btn = toolbar_right_parts.save_btn;
    toolbar.set_end_widget(Some(&toolbar_right_parts.root));

    let crop_ratio_list = GtkBox::new(Orientation::Vertical, 0);
    for crop_type in CropAspectRatio::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label_widget = Label::new(Some(crop_type.label()));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);
        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(crop_type == CropAspectRatio::Freeform);
        check_icon.add_css_class("editor-crop-inspector-check");

        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-crop-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if crop_type == CropAspectRatio::Freeform {
            btn.add_css_class("editor-crop-inspector-option-active");
        }

        crop_ratio_list.append(&btn);
    }

    let crop_dimensions_group = GtkBox::new(Orientation::Vertical, 0);
    crop_dimensions_group.set_halign(gtk4::Align::Fill);
    crop_dimensions_group.set_hexpand(true);

    let crop_dimensions_row = GtkBox::new(Orientation::Horizontal, 8);
    crop_dimensions_row.add_css_class("editor-crop-dimensions-row");
    crop_dimensions_row.set_halign(gtk4::Align::Center);

    // Width box
    let w_box = GtkBox::new(Orientation::Vertical, 0);
    w_box.set_halign(gtk4::Align::Fill);
    w_box.set_hexpand(true);
    w_box.add_css_class("editor-dimension-box");
    let crop_width_value = Label::new(Some("—"));
    crop_width_value.add_css_class("editor-crop-dimensions-value");
    let w_sub_label = Label::new(Some("WIDTH"));
    w_sub_label.add_css_class("editor-dimension-label");
    w_box.append(&crop_width_value);
    w_box.append(&w_sub_label);

    let crop_size_separator = Label::new(Some("×"));
    crop_size_separator.add_css_class("editor-crop-dimensions-separator");
    crop_size_separator.set_valign(gtk4::Align::Center);

    // Height box
    let h_box = GtkBox::new(Orientation::Vertical, 0);
    h_box.set_halign(gtk4::Align::Fill);
    h_box.set_hexpand(true);
    h_box.add_css_class("editor-dimension-box");
    let crop_height_value = Label::new(Some("—"));
    crop_height_value.add_css_class("editor-crop-dimensions-value");
    let h_sub_label = Label::new(Some("HEIGHT"));
    h_sub_label.add_css_class("editor-dimension-label");
    h_box.append(&crop_height_value);
    h_box.append(&h_sub_label);

    crop_dimensions_row.append(&w_box);
    crop_dimensions_row.append(&crop_size_separator);
    crop_dimensions_row.append(&h_box);
    crop_dimensions_group.append(&crop_dimensions_row);

    let crop_actions_group = GtkBox::new(Orientation::Vertical, 8);
    crop_actions_group.set_halign(gtk4::Align::Fill);
    crop_actions_group.set_hexpand(true);

    let crop_apply_btn = Button::with_label("Apply selection");
    crop_apply_btn.set_has_frame(false);
    crop_apply_btn.set_halign(gtk4::Align::Fill);
    crop_apply_btn.set_hexpand(true);
    crop_apply_btn.add_css_class("editor-add-to-colors-button");
    crop_apply_btn.add_css_class("editor-colors-panel-action-button");
    crop_apply_btn.set_sensitive(false);

    let crop_reset_btn = Button::with_label("Reset");
    crop_reset_btn.set_has_frame(false);
    crop_reset_btn.set_halign(gtk4::Align::Fill);
    crop_reset_btn.set_hexpand(true);
    crop_reset_btn.add_css_class("editor-colors-panel-action-button");

    crop_actions_group.append(&crop_apply_btn);
    crop_actions_group.append(&crop_reset_btn);

    let arrow_style_list = GtkBox::new(Orientation::Vertical, 0);
    for style in ArrowStyle::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let style_icon = arrow_style_toolbar_icon(style);
        let icon = tool_icon_widget(style_icon.clone(), toolbar_icon_size(&style_icon));
        let label_widget = Label::new(Some(style.display_name()));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);
        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(style == ArrowStyle::Standard);
        check_icon.add_css_class("editor-arrow-inspector-check");

        btn_box.append(&icon);
        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-arrow-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if style == ArrowStyle::Standard {
            btn.add_css_class("editor-arrow-inspector-option-active");
        }

        arrow_style_list.append(&btn);
    }

    let arrow_thickness_list = GtkBox::new(Orientation::Vertical, 0);
    for (label, _size, weight) in [
        ("Thin", 2.0_f64, PenWeight::Small),
        ("Medium", 4.0_f64, PenWeight::Medium),
        ("Thick", 7.0_f64, PenWeight::Large),
        ("Very Thick", 12.0_f64, PenWeight::ExtraLarge),
    ] {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let icon = build_arrow_thickness_preview(weight);
        let label_widget = Label::new(Some(label));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);
        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(weight == PenWeight::Medium);
        check_icon.add_css_class("editor-arrow-inspector-check");

        btn_box.append(&icon);
        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-arrow-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if weight == PenWeight::Medium {
            btn.add_css_class("editor-arrow-inspector-option-active");
        }

        arrow_thickness_list.append(&btn);
    }

    let arrow_behavior_group = GtkBox::new(Orientation::Vertical, 0);
    arrow_behavior_group.add_css_class("editor-inspector-toggle-row");
    let inverse_direction_toggle = CheckButton::with_label("Reverse direction");
    arrow_behavior_group.append(&inverse_direction_toggle);

    let pen_inspector_list = GtkBox::new(Orientation::Vertical, 0);
    let line_inspector_list = GtkBox::new(Orientation::Vertical, 0);
    let highlighter_inspector_list = GtkBox::new(Orientation::Vertical, 0);
    for weight in PenWeight::ALL {
        let make_button = || {
            let btn_box = GtkBox::new(Orientation::Horizontal, 8);
            btn_box.set_margin_start(8);
            btn_box.set_margin_end(8);
            btn_box.set_margin_top(4);
            btn_box.set_margin_bottom(4);

            let icon = build_arrow_thickness_preview(weight);
            let label_widget = Label::new(Some(weight.label()));
            label_widget.set_hexpand(true);
            label_widget.set_xalign(0.0);
            let check_icon = Label::new(Some("✓"));
            check_icon.set_visible(weight == PenWeight::Medium);
            check_icon.add_css_class("editor-arrow-inspector-check");

            btn_box.append(&icon);
            btn_box.append(&label_widget);
            btn_box.append(&check_icon);

            let btn = Button::builder()
                .has_frame(false)
                .css_classes([
                    "editor-popover-list-item",
                    "flat",
                    "editor-arrow-inspector-option",
                ])
                .child(&btn_box)
                .build();
            if weight == PenWeight::Medium {
                btn.add_css_class("editor-arrow-inspector-option-active");
            }
            btn
        };

        pen_inspector_list.append(&make_button());
        highlighter_inspector_list.append(&make_button());
    }
    for (label, _size, weight) in [
        ("Thin", 2.0_f64, PenWeight::Small),
        ("Medium", 4.0_f64, PenWeight::Medium),
        ("Thick", 7.0_f64, PenWeight::Large),
        ("Very Thick", 12.0_f64, PenWeight::ExtraLarge),
    ] {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let icon = build_arrow_thickness_preview(weight);
        let label_widget = Label::new(Some(label));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);
        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(weight == PenWeight::Medium);
        check_icon.add_css_class("editor-arrow-inspector-check");

        btn_box.append(&icon);
        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-arrow-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if weight == PenWeight::Medium {
            btn.add_css_class("editor-arrow-inspector-option-active");
        }

        line_inspector_list.append(&btn);
    }

    let text_size_list = GtkBox::new(Orientation::Vertical, 0);
    let font_family_list = GtkBox::new(Orientation::Vertical, 0);
    let obfuscate_method_list = GtkBox::new(Orientation::Vertical, 0);

    let number_options_list = GtkBox::new(Orientation::Vertical, 0);
    number_options_list.set_margin_start(4);
    number_options_list.set_margin_end(4);
    number_options_list.set_margin_top(4);
    number_options_list.set_margin_bottom(4);
    for style in super::numbering_style::NumberingStyle::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label = Label::new(Some(style.label()));
        label.set_hexpand(true);
        label.set_xalign(0.0);

        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(style == super::numbering_style::NumberingStyle::default());
        check_icon.add_css_class("editor-number-style-check");

        btn_box.append(&label);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-number-style-option",
            ])
            .child(&btn_box)
            .build();
        if style == super::numbering_style::NumberingStyle::default() {
            btn.add_css_class("editor-number-style-option-active");
        }
        number_options_list.append(&btn);
    }

    let number_start_entry = Entry::new();
    number_start_entry.set_width_chars(5);
    number_start_entry.set_max_width_chars(5);
    number_start_entry.set_text("1");
    number_start_entry.set_editable(false);
    number_start_entry.add_css_class("editor-number-start-entry");
    let number_inc_btn = Button::with_label("+");
    number_inc_btn.add_css_class("editor-number-start-stepper");
    let number_dec_btn = Button::with_label("-");
    number_dec_btn.add_css_class("editor-number-start-stepper");

    let number_start_row = GtkBox::new(Orientation::Horizontal, 8);
    number_start_row.add_css_class("editor-number-start-row");
    let number_start_label = Label::new(Some("Start with:"));
    number_start_label.add_css_class("editor-number-start-label");
    number_start_label.set_hexpand(true);
    number_start_label.set_xalign(0.0);
    number_start_row.append(&number_start_label);
    number_start_row.append(&number_dec_btn);
    number_start_row.append(&number_start_entry);
    number_start_row.append(&number_inc_btn);

    let number_size_list = GtkBox::new(Orientation::Vertical, 0);
    for size in super::numbering_style::NumberSize::ALL {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label = Label::new(Some(size.label()));
        label.set_hexpand(true);
        label.set_xalign(0.0);

        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(size == super::numbering_style::NumberSize::default());
        check_icon.add_css_class("editor-number-size-check");

        btn_box.append(&label);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-number-size-option",
            ])
            .child(&btn_box)
            .build();
        if size == super::numbering_style::NumberSize::default() {
            btn.add_css_class("editor-number-size-option-active");
        }
        number_size_list.append(&btn);
    }

    let footer_parts =
        footer::build_footer(icon_names::COPY_REGULAR, icon_names::CLOUD_ARROW_UP_REGULAR);
    let zoom_button = footer_parts.zoom_button;
    let zoom_label = footer_parts.zoom_label;
    let zoom_header_label = footer_parts.zoom_header_label;
    let zoom_popup = footer_parts.zoom_popup;
    let zoom_minus_btn = footer_parts.zoom_minus_btn;
    let zoom_plus_btn = footer_parts.zoom_plus_btn;
    let zoom_in_btn = footer_parts.zoom_in_btn;
    let zoom_out_btn = footer_parts.zoom_out_btn;
    let fit_to_screen_btn = footer_parts.fit_to_screen_btn;
    let zoom_to_selection_btn = footer_parts.zoom_to_selection_btn;
    let copy_btn = footer_parts.copy_btn;
    let upload_btn = footer_parts.upload_btn;

    let tracked_window_id = next_tracked_window_id("annotate-editor");
    let window_title = "Screenshot Editor";
    let window_namespace = "apexshot-annotate-editor";

    let canvas::CanvasShellParts {
        root: canvas,
        drawing_area,
        canvas_overlay,
        canvas_scroller,
        canvas_eyedropper_ring,
    } = canvas::build_canvas_shell(
        img_width as i32,
        img_height as i32,
        &GtkBox::new(Orientation::Vertical, 0), // Placeholder, will be replaced
        canvas::EYEDROPPER_LOUPE_SIZE,
    );
    // Note: zoom_popup is NOT added to canvas_overlay here - it will be added
    // to a root-level overlay later to stay fixed during zoom/scroll.

    // Background style cache
    let cached_background_surface =
        Rc::new(std::cell::RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let cached_background_style = Rc::new(std::cell::RefCell::new(None::<BackgroundStyle>));
    let cached_blurred_revision = Rc::new(Cell::new(0u64));

    let gradient_surfaces = Rc::new(RefCell::new(vec![
            None::<gtk4::cairo::ImageSurface>;
            background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES.len()
        ]));
    let wallpaper_cache = Rc::new(RefCell::new(std::collections::HashMap::<
        PathBuf,
        gtk4::cairo::ImageSurface,
    >::new()));

    let (wallpaper_loader_sender, receiver) =
        std::sync::mpsc::channel::<(Option<usize>, PathBuf, RgbaImage)>();

    // Pre-load gradients and system wallpaper in background
    {
        let sender = wallpaper_loader_sender.clone();
        // Background loader thread
        std::thread::spawn({
            move || {
                // 1. System wallpaper (High Priority)
                if let Some(path) = background_panel::detect_system_wallpaper_path() {
                    println!("[DEBUG] Detected system wallpaper: {:?}", path);
                    if let Some(rgba) = background_panel::load_background_image_optimized(&path) {
                        let _ = sender.send((None, path, rgba));
                    }
                } else {
                    println!("[DEBUG] No system wallpaper detected.");
                    // Also load the fallback wallpaper into cache
                    let fallback_path = background_panel::background_gradient_asset_path(
                        background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES[0],
                    );
                    if let Some(rgba) =
                        background_panel::load_background_image_optimized(&fallback_path)
                    {
                        let _ = sender.send((None, fallback_path, rgba));
                    }
                }

                // 2. Gradients
                for (idx, file_name) in background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES
                    .iter()
                    .enumerate()
                {
                    let path = background_panel::background_gradient_asset_path(file_name);
                    if let Some(rgba) = background_panel::load_background_image_optimized(&path) {
                        if sender.send((Some(idx), path, rgba)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let gradient_surfaces_main = gradient_surfaces.clone();
        let wallpaper_cache_main = wallpaper_cache.clone();
        let drawing_area_main = drawing_area.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            while let Ok((idx_opt, path, rgba)) = receiver.try_recv() {
                if let Some(surface) = rgba_image_to_surface(&rgba) {
                    if let Some(idx) = idx_opt {
                        gradient_surfaces_main.borrow_mut()[idx] = Some(surface);
                    } else {
                        wallpaper_cache_main.borrow_mut().insert(path, surface);
                    }
                    if let Some(area) = drawing_area_main.upgrade() {
                        area.queue_draw();
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Async Effects Pipeline
    let (effects_sender, effects_receiver) = std::sync::mpsc::channel::<(RgbaImage, u64)>();
    let (request_sender, request_receiver) =
        std::sync::mpsc::channel::<(RgbaImage, Vec<AnnotationAction>, u64)>();

    // Used by the UI thread to coalesce effect rebuild requests.
    let effects_request_sender = request_sender.clone();

    let state_effects = state.clone();
    let drawing_area_effects = drawing_area.downgrade();
    {
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            while let Ok((new_image, revision)) = effects_receiver.try_recv() {
                // Apply results, then if another rebuild was requested while pending,
                // schedule one more rebuild.
                let (should_schedule_next, base_image, actions, next_revision) = {
                    let mut st = state_effects.lock().unwrap();
                    if revision <= st.last_applied_effect_revision {
                        (false, None, None, 0)
                    } else {
                        st.working_image = new_image;
                        st.last_applied_effect_revision = revision;
                        st.select_effect_rebuild_pending = false;
                        st.mark_working_image_dirty();

                        let should = st.select_effect_rebuild_dirty;
                        if should {
                            st.select_effect_rebuild_dirty = false;
                            st.select_effect_rebuild_pending = true;
                            st.pending_effect_revision += 1;
                            (
                                true,
                                Some(st.base_image.clone()),
                                Some(st.actions.clone()),
                                st.pending_effect_revision,
                            )
                        } else {
                            (false, None, None, 0)
                        }
                    }
                };

                if let Some(area) = drawing_area_effects.upgrade() {
                    area.queue_draw();
                }

                if should_schedule_next {
                    if let (Some(base_image), Some(actions)) = (base_image, actions) {
                        let _ = effects_request_sender.send((base_image, actions, next_revision));
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Single background worker thread
    std::thread::spawn(move || {
        while let Ok(mut request) = request_receiver.recv() {
            // Drain the channel to get only the latest request
            while let Ok(newer) = request_receiver.try_recv() {
                request = newer;
            }

            let (base_image, actions, revision) = request;
            let mut working_image = base_image;

            // EXPENSIVE: This blocks the worker thread
            apply_effect_actions(&mut working_image, &actions);

            let _ = effects_sender.send((working_image, revision));
        }
    });

    let rebuild_effects_async: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let sender = request_sender;
        move || {
            let maybe_payload = {
                let mut st = state.lock().unwrap();

                // Avoid flooding the worker with rebuild requests while one is already pending.
                // This helps prevent UI stalls when many effect-triggering actions happen quickly.
                if st.select_effect_rebuild_pending {
                    // A rebuild is already in-flight; remember that we need another pass.
                    st.select_effect_rebuild_dirty = true;
                    return;
                }
                st.select_effect_rebuild_pending = true;
                st.select_effect_rebuild_dirty = false;
                st.last_effect_request_time_us = glib::monotonic_time();

                st.pending_effect_revision += 1;
                Some((
                    st.base_image.clone(),
                    st.actions.clone(),
                    st.pending_effect_revision,
                ))
            };

            if let Some((base_image, actions, revision)) = maybe_payload {
                let _ = sender.send((base_image, actions, revision));
            }
        }
    });

    // Effects rebuild watchdog: if we ever get stuck with `select_effect_rebuild_pending=true`
    // (e.g., app was backgrounded / main loop paused), recover by clearing pending and
    // scheduling a fresh rebuild.
    {
        let state = state.clone();
        let rebuild_effects_async = rebuild_effects_async.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let should_recover = {
                let st = state.lock().unwrap();
                if !st.select_effect_rebuild_pending {
                    false
                } else {
                    let elapsed = glib::monotonic_time() - st.last_effect_request_time_us;
                    // 2 seconds without a result is considered stuck.
                    elapsed > 2_000_000
                }
            };

            if should_recover {
                {
                    let mut st = state.lock().unwrap();
                    st.select_effect_rebuild_pending = false;
                }
                rebuild_effects_async();
            }

            glib::ControlFlow::Continue
        });
    }

    let background_panel_parts = background_panel::build_background_panel(
        &window,
        state.clone(),
        &drawing_area,
        wallpaper_loader_sender,
    );
    let background_inspector = background_panel_parts.root;
    let start_background_gradient_preview_loading =
        background_panel_parts.start_gradient_preview_loading;

    let colors_panel_parts = colors_panel::build_colors_panel(
        state.clone(),
        apply_picker_color_to_editor.clone(),
        custom_slot_colors.clone(),
        refresh_custom_color_slots.clone(),
        Rc::new({
            let sidebar_eyedropper_activation = sidebar_eyedropper_activation.clone();
            move || {
                if let Some(activate) = sidebar_eyedropper_activation.borrow().as_ref() {
                    activate();
                }
            }
        }),
    );
    let colors_inspector = colors_panel_parts.root;
    let sync_colors_panel_for_active_tool = colors_panel_parts.sync_for_active_tool;
    let refresh_colors_panel_custom_slots = colors_panel_parts.refresh_custom_slots;
    let toolbar_color_css_provider = gtk4::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &toolbar_color_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
    let sync_toolbar_color_status: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let color_status_label = color_status_label.clone();
        let _color_status_swatch = color_status_swatch.clone();
        let toolbar_color_css_provider = toolbar_color_css_provider.clone();
        move || {
            let active_color = {
                let st = state.lock().unwrap();
                if st.selected_tool == Tool::Background {
                    if let BackgroundStyle::PlainColor(color) = st.background_style {
                        color
                    } else {
                        st.selected_color
                    }
                } else {
                    st.selected_color
                }
            };

            color_status_label.set_label(&format!("#{}", draw_color_to_hex(active_color)));
            let (r, g, b, _) = draw_color_to_rgba_u8(active_color);
            let alpha = active_color.a.clamp(0.0, 1.0);
            let css = format!(
                "#editor-toolbar-color-status-swatch {{ background: rgba({r}, {g}, {b}, {alpha:.3}); }}"
            );
            toolbar_color_css_provider.load_from_data(&css);
        }
    });
    let sync_shared_colors_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let sync_toolbar_color_status = sync_toolbar_color_status.clone();
        let sync_picker_for_active_tool = sync_picker_for_active_tool.clone();
        let sync_colors_panel_for_active_tool = sync_colors_panel_for_active_tool.clone();
        move || {
            sync_toolbar_color_status();
            sync_picker_for_active_tool();
            sync_colors_panel_for_active_tool();
        }
    });
    register_color_panel_sync(sync_shared_colors_for_active_tool.clone());

    let placeholder_inspector = GtkBox::new(Orientation::Vertical, 12);
    placeholder_inspector.add_css_class("editor-inspector-placeholder-shell");
    placeholder_inspector.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    placeholder_inspector.set_hexpand(false);
    placeholder_inspector.set_vexpand(true);

    let placeholder_title = Label::new(Some("Inspector"));
    placeholder_title.add_css_class("editor-inspector-title");
    placeholder_title.set_xalign(0.0);

    let placeholder = Label::new(Some("Tool options coming soon"));
    placeholder.add_css_class("editor-inspector-placeholder");
    placeholder.set_wrap(true);
    placeholder.set_xalign(0.0);

    placeholder_inspector.append(&placeholder_title);
    placeholder_inspector.append(&placeholder);

    if let Some(parent) = text_size_list.parent() {
        if let Ok(popover) = parent.downcast::<Popover>() {
            popover.set_child(Option::<&gtk4::Widget>::None);
        }
    }
    if let Some(parent) = font_family_list.parent() {
        if let Ok(popover) = parent.downcast::<Popover>() {
            popover.set_child(Option::<&gtk4::Widget>::None);
        }
    }
    if let Some(parent) = arrow_style_list.parent() {
        if let Ok(popover) = parent.downcast::<Popover>() {
            popover.set_child(Option::<&gtk4::Widget>::None);
        }
    }
    if let Some(parent) = number_options_list.parent() {
        if let Ok(popover) = parent.downcast::<Popover>() {
            popover.set_child(Option::<&gtk4::Widget>::None);
        }
    }
    if let Some(parent) = number_size_list.parent() {
        if let Ok(popover) = parent.downcast::<Popover>() {
            popover.set_child(Option::<&gtk4::Widget>::None);
        }
    }
    if let Some(parent) = number_size_button.parent() {
        if let Ok(container) = parent.downcast::<GtkBox>() {
            container.remove(&number_size_button);
        }
    }

    text_size_group.set_visible(false);
    font_family_group.set_visible(false);
    number_options_group.set_visible(false);
    arrow_style_group.set_visible(false);

    let build_tool_inspector = || {
        let root = GtkBox::new(Orientation::Vertical, 12);
        root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
        root.set_hexpand(false);
        root.set_halign(gtk4::Align::Fill);
        root.set_vexpand(true);

        let content = GtkBox::new(Orientation::Vertical, 10);
        content.set_margin_top(4);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.set_hexpand(false);
        content.set_halign(gtk4::Align::Fill);

        root.append(&content);
        (root, content)
    };

    let append_inspector_section = |content: &GtkBox, title: &str, widget: &gtk4::Widget| {
        let section = GtkBox::new(Orientation::Vertical, 8);
        section.add_css_class("editor-inspector-section");

        let section_title = Label::new(Some(title));
        section_title.add_css_class("editor-background-section-title");
        section_title.set_xalign(0.0);

        let section_body = GtkBox::new(Orientation::Vertical, 0);
        section_body.append(widget);

        section.append(&section_title);
        section.append(&section_body);
        content.append(&section);
    };

    let (crop_inspector, crop_inspector_content) = build_tool_inspector();
    crop_ratio_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &crop_inspector_content,
        "Dimensions",
        crop_dimensions_group.upcast_ref(),
    );
    append_inspector_section(
        &crop_inspector_content,
        "Aspect Ratio",
        crop_ratio_list.upcast_ref(),
    );
    append_inspector_section(
        &crop_inspector_content,
        "Actions",
        crop_actions_group.upcast_ref(),
    );

    let (pen_inspector, pen_inspector_content) = build_tool_inspector();
    pen_inspector_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &pen_inspector_content,
        "Thickness",
        pen_inspector_list.upcast_ref(),
    );

    let (arrow_inspector, arrow_inspector_content) = build_tool_inspector();
    arrow_style_list.add_css_class("editor-inspector-option-list");
    arrow_thickness_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &arrow_inspector_content,
        "Style",
        arrow_style_list.upcast_ref(),
    );
    append_inspector_section(
        &arrow_inspector_content,
        "Thickness",
        arrow_thickness_list.upcast_ref(),
    );
    append_inspector_section(
        &arrow_inspector_content,
        "Behavior",
        arrow_behavior_group.upcast_ref(),
    );

    let (line_inspector, line_inspector_content) = build_tool_inspector();
    line_inspector_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &line_inspector_content,
        "Thickness",
        line_inspector_list.upcast_ref(),
    );

    let (text_inspector, text_inspector_content) = build_tool_inspector();
    text_size_list.add_css_class("editor-inspector-option-list");
    font_family_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(&text_inspector_content, "Size", text_size_list.upcast_ref());
    append_inspector_section(
        &text_inspector_content,
        "Font",
        font_family_list.upcast_ref(),
    );

    let (obfuscate_inspector, obfuscate_inspector_content) = build_tool_inspector();
    obfuscate_method_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &obfuscate_inspector_content,
        "Method",
        obfuscate_method_list.upcast_ref(),
    );

    let (number_inspector, number_inspector_content) = build_tool_inspector();
    number_options_list.add_css_class("editor-inspector-option-list");
    number_size_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &number_inspector_content,
        "Style",
        number_options_list.upcast_ref(),
    );
    append_inspector_section(
        &number_inspector_content,
        "Start",
        number_start_row.upcast_ref(),
    );
    append_inspector_section(
        &number_inspector_content,
        "Size",
        number_size_list.upcast_ref(),
    );

    let (highlighter_inspector, highlighter_inspector_content) = build_tool_inspector();
    highlighter_inspector_list.add_css_class("editor-inspector-option-list");
    append_inspector_section(
        &highlighter_inspector_content,
        "Thickness",
        highlighter_inspector_list.upcast_ref(),
    );

    let inspector_tabs = GtkBox::new(Orientation::Horizontal, 8);
    inspector_tabs.add_css_class("editor-inspector-tabs");
    inspector_tabs.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector_tabs.set_hexpand(false);
    inspector_tabs.set_halign(gtk4::Align::Fill);

    let background_tab_btn = Button::with_label("Background");
    background_tab_btn.set_has_frame(false);
    background_tab_btn.add_css_class("editor-inspector-tab-button");

    let colors_tab_btn = Button::with_label("Colors");
    colors_tab_btn.set_has_frame(false);
    colors_tab_btn.add_css_class("editor-inspector-tab-button");

    inspector_tabs.append(&background_tab_btn);
    inspector_tabs.append(&colors_tab_btn);

    let inspector = GtkBox::new(Orientation::Vertical, 0);
    inspector.add_css_class("editor-right-inspector");
    inspector.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector.set_hexpand(false);
    inspector.set_vexpand(true);
    inspector.append(&inspector_tabs);

    let inspector_stack = Stack::new();
    inspector_stack.set_hhomogeneous(true);
    inspector_stack.set_vhomogeneous(false);
    inspector_stack.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    inspector_stack.set_hexpand(false);
    inspector_stack.set_vexpand(true);
    background_inspector.set_visible(true);
    crop_inspector.set_visible(true);
    pen_inspector.set_visible(true);
    arrow_inspector.set_visible(true);
    line_inspector.set_visible(true);
    text_inspector.set_visible(true);
    highlighter_inspector.set_visible(true);
    obfuscate_inspector.set_visible(true);
    number_inspector.set_visible(true);
    colors_inspector.set_visible(true);
    placeholder_inspector.set_visible(true);
    inspector_stack.add_named(&background_inspector, Some("background"));
    inspector_stack.add_named(&crop_inspector, Some("crop"));
    inspector_stack.add_named(&pen_inspector, Some("pen"));
    inspector_stack.add_named(&arrow_inspector, Some("arrow"));
    inspector_stack.add_named(&line_inspector, Some("line"));
    inspector_stack.add_named(&text_inspector, Some("text"));
    inspector_stack.add_named(&highlighter_inspector, Some("highlighter"));
    inspector_stack.add_named(&obfuscate_inspector, Some("obfuscate"));
    inspector_stack.add_named(&number_inspector, Some("number"));
    inspector_stack.add_named(&colors_inspector, Some("colors"));
    inspector_stack.add_named(&placeholder_inspector, Some("placeholder"));
    inspector_stack.set_visible_child_name("placeholder");
    inspector.append(&inspector_stack);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.append(&canvas);
    workspace.append(&inspector);

    *drawing_area_placeholder.borrow_mut() = Some(drawing_area.downgrade());

    let sync_inspector_thickness_controls: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let crop_ratio_list = crop_ratio_list.clone();
        let crop_apply_btn = crop_apply_btn.clone();
        let crop_width_value = crop_width_value.clone();
        let crop_height_value = crop_height_value.clone();
        let pen_inspector_list = pen_inspector_list.clone();
        let arrow_style_list = arrow_style_list.clone();
        let arrow_thickness_list = arrow_thickness_list.clone();
        let line_inspector_list = line_inspector_list.clone();
        let highlighter_inspector_list = highlighter_inspector_list.clone();
        let inverse_direction_toggle = inverse_direction_toggle.clone();
        move || {
            let st = state.lock().unwrap();
            let selected_ratio = CropAspectRatio::ALL
                .iter()
                .position(|ratio| *ratio == st.crop_aspect_ratio)
                .unwrap_or(0);
            sync_crop_option_selection(&crop_ratio_list, selected_ratio);
            if let Some(rect) = st.draft_crop_rect().or(st.crop_selection) {
                crop_width_value.set_label(&rect.width.max(0).to_string());
                crop_height_value.set_label(&rect.height.max(0).to_string());
            } else {
                crop_width_value.set_label("—");
                crop_height_value.set_label("—");
            }
            crop_apply_btn
                .set_sensitive(st.draft_crop_rect().is_some() || st.crop_selection.is_some());
            let selected_style_value = st.selected_arrow_style().unwrap_or(st.arrow_style);
            let selected_style = ArrowStyle::ALL
                .iter()
                .position(|style| *style == selected_style_value)
                .unwrap_or(0);
            let selected_stroke_size = st.selected_action_stroke_size().unwrap_or(st.stroke_size);
            let selected_thickness = stroke_size_option_index(selected_stroke_size);
            let selected_pen_thickness = pen_weight_option_index(selected_stroke_size);
            sync_arrow_option_selection(&pen_inspector_list, selected_pen_thickness);
            sync_arrow_option_selection(&arrow_style_list, selected_style);
            sync_arrow_option_selection(&arrow_thickness_list, selected_thickness);
            sync_arrow_option_selection(&line_inspector_list, selected_thickness);
            sync_arrow_option_selection(&highlighter_inspector_list, selected_pen_thickness);
            inverse_direction_toggle.set_active(st.inverse_arrow_direction);
        }
    });

    let update_toolbar_for_tool_base = toolbar::build_toolbar_tool_updater(
        &toolbar_mode_stack,
        &inspector_stack,
        &inspector_tabs,
        &background_tab_btn,
        &colors_tab_btn,
        &text_size_group,
        &font_family_group,
        &obfuscate_method_group,
        &pen_weight_group,
        &number_options_group,
        &arrow_style_group,
        &stroke_size_group,
        &canvas_scroller,
        start_background_gradient_preview_loading.clone(),
    );
    let update_toolbar_for_tool: Rc<dyn Fn(Tool)> = Rc::new({
        let update_toolbar_for_tool_base = update_toolbar_for_tool_base.clone();
        let sync_inspector_thickness_controls = sync_inspector_thickness_controls.clone();
        move |tool| {
            update_toolbar_for_tool_base(tool);
            if matches!(
                tool,
                Tool::Crop | Tool::Pen | Tool::Arrow | Tool::Line | Tool::Highlighter
            ) {
                sync_inspector_thickness_controls();
            }
        }
    });

    let set_active_inspector_surface: Rc<dyn Fn(&str)> = Rc::new({
        let inspector_stack = inspector_stack.clone();
        let background_tab_btn = background_tab_btn.clone();
        let colors_tab_btn = colors_tab_btn.clone();
        move |surface| {
            let show_background = !matches!(surface, "colors" | "placeholder");
            let show_colors = surface == "colors";
            inspector_stack.set_visible_child_name(surface);
            if show_background {
                background_tab_btn.add_css_class("active-inspector-tab");
            } else {
                background_tab_btn.remove_css_class("active-inspector-tab");
            }
            if show_colors {
                colors_tab_btn.add_css_class("active-inspector-tab");
            } else {
                colors_tab_btn.remove_css_class("active-inspector-tab");
            }
        }
    });

    background_tab_btn.connect_clicked({
        let state = state.clone();
        let set_active_inspector_surface = set_active_inspector_surface.clone();
        move |_| {
            let surface = match state.lock().unwrap().selected_tool {
                Tool::Background => Some("background"),
                Tool::Crop => Some("crop"),
                Tool::Pen => Some("pen"),
                Tool::Arrow => Some("arrow"),
                Tool::Line => Some("line"),
                Tool::Text => Some("text"),
                Tool::Number => Some("number"),
                Tool::Highlighter => Some("highlighter"),
                _ => None,
            };
            if let Some(surface) = surface {
                set_active_inspector_surface(surface);
            }
        }
    });

    colors_tab_btn.connect_clicked({
        let state = state.clone();
        let set_active_inspector_surface = set_active_inspector_surface.clone();
        let sync_colors_panel_for_active_tool = sync_colors_panel_for_active_tool.clone();
        move |_| {
            let selected_tool = state.lock().unwrap().selected_tool;
            if matches!(
                selected_tool,
                Tool::Background
                    | Tool::Crop
                    | Tool::Pen
                    | Tool::Arrow
                    | Tool::Line
                    | Tool::Box
                    | Tool::Circle
                    | Tool::Text
                    | Tool::Number
                    | Tool::Highlighter
                    | Tool::Obfuscate
                    | Tool::Focus
            ) {
                sync_colors_panel_for_active_tool();
                set_active_inspector_surface("colors");
            }
        }
    });

    let canvas_padding = canvas::CANVAS_PADDING;

    let update_crop_size_fields: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let crop_width_value = crop_width_value.clone();
        let crop_height_value = crop_height_value.clone();
        move || {
            let st = state.lock().unwrap();
            if let Some(rect) = st.draft_crop_rect().or(st.crop_selection) {
                crop_width_value.set_label(&rect.width.max(0).to_string());
                crop_height_value.set_label(&rect.height.max(0).to_string());
            } else {
                crop_width_value.set_label("—");
                crop_height_value.set_label("—");
            }
        }
    });

    let mut crop_type_index = 0usize;
    let mut crop_child_opt = crop_ratio_list.first_child();
    while let Some(child) = crop_child_opt {
        crop_child_opt = child.next_sibling();
        let Ok(option_button) = child.downcast::<Button>() else {
            continue;
        };

        let Some(&crop_type) = CropAspectRatio::ALL.get(crop_type_index) else {
            break;
        };
        let selected_index = crop_type_index;
        crop_type_index += 1;

        let crop_ratio_list_option = crop_ratio_list.clone();
        let state_crop_type_option = state.clone();
        let drawing_area_crop_type_option = drawing_area.downgrade();
        let update_crop_size_fields_option = update_crop_size_fields.clone();
        let crop_apply_btn_option = crop_apply_btn.clone();
        option_button.connect_clicked(move |_| {
            {
                let mut st = state_crop_type_option.lock().unwrap();
                st.set_crop_aspect_ratio(crop_type);
                if st.selected_tool == Tool::Crop {
                    st.ensure_crop_selection_initialized();
                }
                crop_apply_btn_option
                    .set_sensitive(st.draft_crop_rect().is_some() || st.crop_selection.is_some());
            }
            sync_crop_option_selection(&crop_ratio_list_option, selected_index);
            update_crop_size_fields_option();
            if let Some(area) = drawing_area_crop_type_option.upgrade() {
                area.queue_draw();
            }
        });
    }

    let (selected_text_size, selected_font_family) = {
        let st = state.lock().unwrap();
        (
            st.selected_text_action_size()
                .map(|size| size as i32)
                .unwrap_or(st.text_size as i32),
            st.selected_text_font_family()
                .unwrap_or_else(|| st.text_font_family.clone()),
        )
    };

    while let Some(child) = text_size_list.first_child() {
        text_size_list.remove(&child);
    }
    for size in TEXT_SIZE_OPTIONS {
        let label = format!("{}pt", size);
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label_widget = Label::new(Some(&label));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);

        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(size == selected_text_size);
        check_icon.add_css_class("editor-text-inspector-check");

        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-text-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if size == selected_text_size {
            btn.add_css_class("editor-text-inspector-option-active");
        }
        let state = state.clone();
        let text_size_label = text_size_label.clone();
        let text_size_list_sync = text_size_list.clone();
        let drawing_area = drawing_area.clone();
        btn.connect_clicked(move |b| {
            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }
            text_size_label.set_label(&format!("{}pt", size));
            let mut st = state.lock().unwrap();
            let changed = st.set_text_size(size as f64);
            let has_active_text = st.active_text_input.is_some();
            if !changed && st.active_text_input.is_none() && st.selected_action_index.is_none() {
                st.text_size = size as f64;
            }
            drop(st);
            sync_text_option_selection(
                &text_size_list_sync,
                TEXT_SIZE_OPTIONS
                    .iter()
                    .position(|candidate| *candidate == size),
            );
            if has_active_text {
                drawing_area.grab_focus();
            }
            drawing_area.queue_draw();
        });
        text_size_list.append(&btn);
    }

    while let Some(child) = font_family_list.first_child() {
        font_family_list.remove(&child);
    }
    for family in TEXT_FONT_FAMILIES {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label_widget = Label::new(Some(family));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);

        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(family == selected_font_family);
        check_icon.add_css_class("editor-text-inspector-check");

        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-text-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if family == selected_font_family {
            btn.add_css_class("editor-text-inspector-option-active");
        }
        let state = state.clone();
        let font_family_label = font_family_label.clone();
        let font_family_list_sync = font_family_list.clone();
        let drawing_area = drawing_area.clone();
        let family_str = family.to_string();
        btn.connect_clicked(move |b| {
            if let Some(popover) = b.ancestor(Popover::static_type()) {
                popover.downcast::<Popover>().unwrap().popdown();
            }
            font_family_label.set_label(&family_str);
            let mut st = state.lock().unwrap();
            let changed = st.set_selected_text_font_family(family_str.clone());
            let has_active_text = st.active_text_input.is_some();
            if st.active_text_input.is_some() {
                st.text_font_family = family_str.clone();
            } else if !changed {
                st.text_font_family = family_str.clone();
            }
            drop(st);
            sync_text_option_selection(
                &font_family_list_sync,
                TEXT_FONT_FAMILIES
                    .iter()
                    .position(|candidate| *candidate == family_str.as_str()),
            );
            if has_active_text {
                drawing_area.grab_focus();
            }
            drawing_area.queue_draw();
        });
        font_family_list.append(&btn);
    }

    while let Some(child) = obfuscate_method_list.first_child() {
        obfuscate_method_list.remove(&child);
    }
    let selected_obfuscate_method = {
        let st = state.lock().unwrap();
        st.obfuscate_method()
    };
    for (index, (method, label)) in OBFUSCATE_METHOD_OPTIONS.iter().enumerate() {
        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_start(8);
        btn_box.set_margin_end(8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);

        let label_widget = Label::new(Some(label));
        label_widget.set_hexpand(true);
        label_widget.set_xalign(0.0);

        let check_icon = Label::new(Some("✓"));
        check_icon.set_visible(*method == selected_obfuscate_method);
        check_icon.add_css_class("editor-obfuscate-inspector-check");

        btn_box.append(&label_widget);
        btn_box.append(&check_icon);

        let btn = Button::builder()
            .has_frame(false)
            .css_classes([
                "editor-popover-list-item",
                "flat",
                "editor-obfuscate-inspector-option",
            ])
            .child(&btn_box)
            .build();
        if *method == selected_obfuscate_method {
            btn.add_css_class("editor-obfuscate-inspector-option-active");
        }

        let state = state.clone();
        let drawing_area = drawing_area.clone();
        let obfuscate_method_list_sync = obfuscate_method_list.clone();
        btn.connect_clicked(move |_| {
            {
                let mut st = state.lock().unwrap();
                st.set_obfuscate_method(*method);
            }
            sync_obfuscate_option_selection(&obfuscate_method_list_sync, index);
            drawing_area.queue_draw();
        });

        obfuscate_method_list.append(&btn);
    }

    let eyedropper_mode = Rc::new(Cell::new(false));
    let eyedropper_from_sidebar = Rc::new(Cell::new(false));
    let eyedropper_point = Rc::new(RefCell::new(None::<Point>));
    let eyedropper_rendered = Rc::new(RefCell::new(None::<RgbaImage>));

    *sidebar_eyedropper_activation.borrow_mut() = Some(Rc::new({
        let color_popover = color_popover.clone();
        let state = state.clone();
        let eyedropper_mode = eyedropper_mode.clone();
        let eyedropper_from_sidebar = eyedropper_from_sidebar.clone();
        let eyedropper_point = eyedropper_point.clone();
        let eyedropper_rendered = eyedropper_rendered.clone();
        let canvas_eyedropper_ring = canvas_eyedropper_ring.clone();
        let drawing_area = drawing_area.clone();
        let window = window.downgrade();
        move || {
            eyedropper_from_sidebar.set(true);
            color_picker::activate_eyedropper(
                &color_popover,
                state.clone(),
                eyedropper_mode.clone(),
                eyedropper_point.clone(),
                eyedropper_rendered.clone(),
                &canvas_eyedropper_ring,
                &drawing_area,
                Rc::new({
                    let window = window.clone();
                    move || {
                        if let Some(window) = window.upgrade() {
                            cursor::set_window_cursor_name(&window, Some("crosshair"));
                        }
                    }
                }),
            );
        }
    }));

    {
        let state_text_blink = state.clone();
        let drawing_area_text_blink = drawing_area.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let has_active_text = {
                let mut st = state_text_blink.lock().unwrap();
                if st.active_text_input.is_none() {
                    false
                } else {
                    st.tick_cursor_blink();
                    true
                }
            };
            if has_active_text {
                if let Some(area) = drawing_area_text_blink.upgrade() {
                    area.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    canvas_eyedropper_ring.set_draw_func({
        let eyedropper_point_draw = eyedropper_point.clone();
        let eyedropper_rendered_draw = eyedropper_rendered.clone();
        move |_, context, width, height| {
            let Some(point) = *eyedropper_point_draw.borrow() else {
                return;
            };

            let rendered = eyedropper_rendered_draw.borrow();
            let Some(rendered) = rendered.as_ref() else {
                return;
            };

            canvas::draw_eyedropper_loupe(context, width, height, rendered, point);
        }
    });

    root.append(&toolbar);
    root.append(&workspace);
    root.append(&footer_parts.root);

    // Wrap root in an overlay so zoom_popup can be positioned on top
    // without being affected by canvas zoom/scroll transformations
    let root_overlay = Overlay::new();
    root_overlay.set_child(Some(&root));
    root_overlay.add_overlay(&zoom_popup);
    window.set_child(Some(&root_overlay));

    // Enable window drag from toolbar (empty areas only) and edge resize
    install_window_drag(&toolbar, &window);
    install_edge_resize(&root_overlay, &window);

    let update_canvas_content_size: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let zoom_level = zoom_level.clone();
        let zoom_label = zoom_label.clone();
        let zoom_header_label = zoom_header_label.clone();
        let drawing_area = drawing_area.clone();
        let _canvas_overlay = canvas_overlay.clone();
        let canvas_scroller = canvas_scroller.clone();
        let _window = window.downgrade();
        let canvas_padding = canvas_padding;
        move || {
            let (
                image_w,
                image_h,
                background_padding,
                background_aspect_ratio,
                has_background,
                crop_rect,
                crop_mode_active,
            ) = {
                let st = state.lock().unwrap();
                (
                    st.working_image.width().max(1) as i32,
                    st.working_image.height().max(1) as i32,
                    st.background_padding,
                    st.background_aspect_ratio,
                    st.background_style != BackgroundStyle::None,
                    st.draft_crop_rect().or(st.crop_selection),
                    st.selected_tool == Tool::Crop,
                )
            };

            let mut virtual_w = image_w as f64;
            let mut virtual_h = image_h as f64;

            if has_background {
                let ref_size = virtual_w.max(virtual_h);
                let scale_factor = ref_size / 400.0;
                let padding_px = background_padding * scale_factor;
                virtual_w += padding_px * 2.0;
                virtual_h += padding_px * 2.0;

                if let Some(ratio) =
                    background_aspect_ratio.aspect_ratio(virtual_w as i32, virtual_h as i32)
                {
                    let current_ratio = virtual_w / virtual_h;
                    if current_ratio < ratio {
                        virtual_w = virtual_h * ratio;
                    } else {
                        virtual_h = virtual_w / ratio;
                    }
                }
            }

            let scroller_width = canvas_scroller.allocated_width().max(1) as f64;
            let scroller_height = canvas_scroller.allocated_height().max(1) as f64;
            let available_width = (scroller_width - (canvas_padding * 2 + 2) as f64).max(1.0);
            let available_height = (scroller_height - (canvas_padding * 2 + 2) as f64).max(1.0);

            // Use the minimum of width and height to maintain aspect ratio and prevent asymmetric growth
            let available_size = available_width.min(available_height);

            // Layout scale without zoom - used for content size (prevents window from growing on zoom)
            let layout_scale = (available_size / virtual_w.min(virtual_h)).min(1.0_f64);
            // Rendering scale includes zoom for visual display
            let scale = layout_scale * zoom_level.get().max(0.1_f64);

            let fitted_w = (virtual_w * scale).round().max(1.0) as i32;
            let fitted_h = (virtual_h * scale).round().max(1.0) as i32;

            let (overflow_left, overflow_top, overflow_right, overflow_bottom) = if has_background {
                (0.0, 0.0, 0.0, 0.0)
            } else {
                canvas::crop_canvas_overflow(
                    crop_rect,
                    image_w as f64,
                    image_h as f64,
                    scale,
                    crop_mode_active,
                )
            };

            let canvas_w = fitted_w
                + canvas_padding * 2
                + overflow_left.round() as i32
                + overflow_right.round() as i32;
            let canvas_h = fitted_h
                + canvas_padding * 2
                + overflow_top.round() as i32
                + overflow_bottom.round() as i32;

            drawing_area.set_content_width(canvas_w);
            drawing_area.set_content_height(canvas_h);
            let percent_str = format!("{}%", (scale * 100.0).round().max(1.0) as i32);
            zoom_label.set_label(&percent_str);
            zoom_header_label.set_label(&percent_str);
        }
    });
    update_canvas_content_size();

    {
        let update_canvas_content_size_tick = update_canvas_content_size.clone();
        let state_canvas_tick = state.clone();
        let zoom_level_tick = zoom_level.clone();
        // Signature tracks the quantities that actually change the *visible* canvas size.
        // Crucially, raw crop-rect coordinates are NOT included here.  Instead we compute
        // the capped overflow bucket that crop_canvas_overflow() would return and store
        // only that.  Because the function caps every side to 180 px, the bucket stays
        // constant throughout an outside-image drag gesture — no relayout churn occurs.
        let last_canvas_signature = Rc::new(Cell::new((
            0_i32, // scroller width
            0_i32, // scroller height
            0_i32, // image width
            0_i32, // image height
            0_i32, // overflow left (px, capped)
            0_i32, // overflow top  (px, capped)
            0_i32, // overflow right (px, capped)
            0_i32, // overflow bottom (px, capped)
            false, // crop mode active
            0_i32, // zoom percentage
        )));
        let last_canvas_signature_tick = last_canvas_signature.clone();
        canvas_scroller.add_tick_callback(move |scroller, _| {
            let width = scroller.allocated_width();
            let height = scroller.allocated_height();
            let signature = {
                let st = state_canvas_tick.lock().unwrap();
                let img_w = st.working_image.width().max(1) as i32;
                let img_h = st.working_image.height().max(1) as i32;
                let crop_mode_active = st.selected_tool == Tool::Crop;
                let crop_rect = st.draft_crop_rect().or(st.crop_selection);
                let has_background = st.background_style != BackgroundStyle::None;
                let zoom_percentage = (zoom_level_tick.get() * 100.0_f64).round() as i32;

                // Compute the same scale the layout function uses so we get the
                // same overflow values without duplicating the full layout calculation.
                let virtual_w = img_w as f64;
                let virtual_h = img_h as f64;
                let available_w = (width as f64 - (canvas_padding * 2 + 2) as f64).max(1.0);
                let available_h = (height as f64 - (canvas_padding * 2 + 2) as f64).max(1.0);

                let available_size = available_w.min(available_h);
                let layout_scale = (available_size / virtual_w.min(virtual_h)).min(1.0_f64);
                let _scale = layout_scale * zoom_level_tick.get().max(0.1_f64);

                let (ol, ot, or_, ob) = if has_background {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    canvas::crop_canvas_overflow(
                        crop_rect,
                        img_w as f64,
                        img_h as f64,
                        layout_scale,
                        crop_mode_active,
                    )
                };

                (
                    width,
                    height,
                    img_w,
                    img_h,
                    ol.round() as i32,
                    ot.round() as i32,
                    or_.round() as i32,
                    ob.round() as i32,
                    crop_mode_active,
                    zoom_percentage,
                )
            };
            if width > 0 && signature != last_canvas_signature_tick.get() {
                last_canvas_signature_tick.set(signature);
                update_canvas_content_size_tick();
            }
            glib::ControlFlow::Continue
        });
    }

    // Eyedropper
    color_picker::connect_eyedropper_activation(
        &eyedropper_btn,
        &color_popover,
        state.clone(),
        eyedropper_mode.clone(),
        eyedropper_point.clone(),
        eyedropper_rendered.clone(),
        &canvas_eyedropper_ring,
        &drawing_area,
        Rc::new({
            let window = window.downgrade();
            move || {
                if let Some(window) = window.upgrade() {
                    cursor::set_window_cursor_name(&window, Some("crosshair"));
                }
            }
        }),
    );

    // Drawing area draw function
    let cached_surface = Rc::new(std::cell::RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let cached_surface_revision = Rc::new(Cell::new(0_u64));

    let sync_size_control: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let size_group = size_group.clone();
        let size_slider = size_slider.clone();
        let text_size_label = text_size_label.clone();
        let font_family_label = font_family_label.clone();
        let text_size_list = text_size_list.clone();
        let font_family_list = font_family_list.clone();
        let obfuscate_method_list = obfuscate_method_list.clone();
        move || {
            // Extract all needed data BEFORE any GTK operations to avoid deadlock
            let (selected_tool, mode, value, text_size, font_family, obfuscate_method) = {
                let st = state.lock().unwrap();
                (
                    st.selected_tool,
                    st.active_size_control_mode(),
                    st.active_size_value().unwrap_or_default(),
                    st.text_size,
                    st.text_font_family.clone(),
                    st.obfuscate_method(),
                )
            };

            text_size_label.set_label(&format!("{}pt", text_size as i32));
            font_family_label.set_label(&font_family);
            sync_text_option_selection(
                &text_size_list,
                TEXT_SIZE_OPTIONS
                    .iter()
                    .position(|candidate| *candidate == text_size as i32),
            );
            sync_text_option_selection(
                &font_family_list,
                TEXT_FONT_FAMILIES
                    .iter()
                    .position(|candidate| *candidate == font_family.as_str()),
            );
            if let Some(selected_method) = OBFUSCATE_METHOD_OPTIONS
                .iter()
                .position(|(method, _)| *method == obfuscate_method)
            {
                sync_obfuscate_option_selection(&obfuscate_method_list, selected_method);
            }

            // Now perform GTK operations WITHOUT holding the lock
            if selected_tool == Tool::Highlighter {
                size_group.set_visible(true);
                size_group.add_css_class("size-group-inactive");
                size_slider.set_tooltip_text(Some("Use the Thickness panel for highlighter"));
                size_slider.set_sensitive(false);
                return;
            }

            size_group.set_visible(true);

            let Some(mode) = mode else {
                size_group.add_css_class("size-group-inactive");
                size_slider.set_tooltip_text(Some("Current tool does not support size changes"));
                size_slider.set_sensitive(false);
                return;
            };

            size_group.remove_css_class("size-group-inactive");
            size_slider.set_sensitive(true);

            use super::color::{MAX_STROKE_SIZE, MIN_STROKE_SIZE};
            use super::types::SizeControlMode;
            match mode {
                SizeControlMode::Stroke => {
                    size_slider.set_range(MIN_STROKE_SIZE, MAX_STROKE_SIZE);
                    size_slider.set_value(value);
                    size_slider.set_tooltip_text(Some("Stroke size"));
                }
                SizeControlMode::Obfuscate => {
                    use super::color::{MAX_OBFUSCATE_AMOUNT, MIN_OBFUSCATE_AMOUNT};
                    // Blackout has no intensity — hide the slider.
                    // For all other methods, enable it with the per-method current value.
                    let method = {
                        let st = state.lock().unwrap();
                        st.obfuscate_method
                    };
                    if matches!(method, super::types::ObfuscateMethod::Blackout) {
                        size_group.set_visible(false);
                    } else {
                        size_group.set_visible(true);
                        size_group.remove_css_class("size-group-inactive");
                        size_slider.set_sensitive(true);
                        size_slider.set_range(MIN_OBFUSCATE_AMOUNT, MAX_OBFUSCATE_AMOUNT);
                        size_slider.set_value(value);
                        let tooltip = match method {
                            super::types::ObfuscateMethod::Pixelate => "Pixelate intensity",
                            super::types::ObfuscateMethod::BlurSecure => "Blur (Secure) intensity",
                            super::types::ObfuscateMethod::BlurSmooth => "Blur (Smooth) intensity",
                            super::types::ObfuscateMethod::Blackout => "Blackout",
                        };
                        size_slider.set_tooltip_text(Some(tooltip));
                    }
                }
            }
        }
    });
    sync_size_control();
    let initial_tool = state.lock().unwrap().selected_tool;
    update_toolbar_for_tool(initial_tool);

    let state_draw = state.clone();
    let transform_draw = transform.clone();
    let zoom_level_draw = zoom_level.clone();
    let undo_btn_draw = undo_btn.clone();
    let redo_btn_draw = redo_btn.clone();
    let delete_selected_btn_draw = delete_selected_btn.clone();
    let cached_surface_draw = cached_surface.clone();
    let cached_surface_revision_draw = cached_surface_revision.clone();
    let cached_background_surface_draw = cached_background_surface.clone();
    let cached_background_style_draw = cached_background_style.clone();
    let cached_blurred_revision_draw = cached_blurred_revision.clone();
    let canvas_padding_draw = canvas_padding as f64;
    let gradient_surfaces_draw = gradient_surfaces.clone();
    let wallpaper_cache_draw = wallpaper_cache.clone();
    drawing_area.set_draw_func(move |_, context, width, height| {
        // IMPORTANT: do not hold the state mutex while performing cairo drawing.
        // The async effects pipeline also locks this mutex on the GTK thread to apply results;
        // holding it here can cause UI stalls/deadlocks.
        let (
            can_undo,
            can_redo,
            can_delete,
            working_image,
            working_image_revision,
            actions,
            draft_action,
            crop_rect,
            crop_mode_active,
            crop_background_color_explicit,
            crop_background_color,
            background_style,
            background_padding,
            background_aspect_ratio,
            background_insert,
            background_alignment,
            background_shadow,
            background_corner_radius,
            selected_tool,
            selected_action,
            select_drag_anchor,
            select_resize_handle,
            active_text_bounds,
            active_text_input,
            active_text_drag_handle,
            text_font_family,
            text_size,
            hovered_text_action_index,
            arrow_editing_controls,
        ) = {
            let st = state_draw.lock().unwrap();
            let (can_undo, can_redo) = st.history_availability();
            (
                can_undo,
                can_redo,
                st.can_remove_selected_action(),
                st.working_image.clone(),
                st.working_image_revision,
                st.actions.clone(),
                st.draft_action(),
                if st.selected_tool == Tool::Crop {
                    st.draft_crop_rect().or(st.crop_selection)
                } else {
                    None
                },
                st.selected_tool == Tool::Crop,
                st.crop_background_color_explicit,
                st.crop_background_color,
                st.background_style.clone(),
                st.background_padding,
                st.background_aspect_ratio,
                st.background_insert,
                st.background_alignment,
                st.background_shadow,
                st.background_corner_radius,
                st.selected_tool,
                st.selected_action().cloned(),
                st.select_drag_anchor,
                st.select_resize_handle,
                st.active_text_bounds.clone(),
                st.active_text_input.clone(),
                st.active_text_drag_handle.clone(),
                st.text_font_family.clone(),
                st.text_size,
                st.hovered_text_action_index,
                st.arrow_editing_controls,
            )
        };

        undo_btn_draw.set_sensitive(can_undo);
        redo_btn_draw.set_sensitive(can_redo);
        delete_selected_btn_draw.set_sensitive(can_delete);

        let image_width = working_image.width() as f64;
        let image_height = working_image.height() as f64;
        let crop_mode_active = crop_mode_active;

        let mut virtual_w = image_width;
        let mut virtual_h = image_height;
        let mut padding_px = 0.0;
        let mut draw_scale_factor = 1.0;

        let has_background = background_style != BackgroundStyle::None;
        if has_background {
            let ref_size = image_width.max(image_height);
            let scale_factor = ref_size / 400.0;
            padding_px = background_padding * scale_factor;

            virtual_w = image_width + padding_px * 2.0;
            virtual_h = image_height + padding_px * 2.0;

            if let Some(ratio) =
                background_aspect_ratio.aspect_ratio(virtual_w as i32, virtual_h as i32)
            {
                let current_ratio = virtual_w / virtual_h;
                if current_ratio < ratio {
                    virtual_w = virtual_h * ratio;
                } else {
                    virtual_h = virtual_w / ratio;
                }
            }

            let insert_ratio = background_insert / 200.0;
            draw_scale_factor = 1.0 - insert_ratio;
        }

        let base_view_width = (width as f64 - canvas_padding_draw * 2.0).max(1.0);
        let base_scale = (base_view_width / virtual_w).min(1.0);
        let (overflow_left, overflow_top, overflow_right, overflow_bottom) = if has_background {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            canvas::crop_canvas_overflow(
                crop_rect,
                image_width,
                image_height,
                base_scale,
                crop_mode_active,
            )
        };

        let view_width =
            (width as f64 - canvas_padding_draw * 2.0 - overflow_left - overflow_right).max(1.0);
        let view_height =
            (height as f64 - canvas_padding_draw * 2.0 - overflow_top - overflow_bottom).max(1.0);

        let scale = (view_width / virtual_w)
            .min(view_height / virtual_h)
            .min(1.0_f64)
            * zoom_level_draw.get().max(0.1_f64);
        let draw_width = virtual_w * scale;
        let draw_height = virtual_h * scale;
        let mut t = ViewTransform {
            scale,
            offset_x: (view_width - draw_width) / 2.0 + canvas_padding_draw + overflow_left,
            offset_y: (view_height - draw_height) / 2.0 + canvas_padding_draw + overflow_top,
            image_width: virtual_w,
            image_height: virtual_h,
        };

        let canvas_t = t.clone();

        context.set_operator(gtk4::cairo::Operator::Source);
        draw_canvas_checkerboard_background(
            context,
            width,
            height,
            if crop_mode_active && crop_background_color_explicit {
                Some(crop_background_color)
            } else {
                None
            },
        );

        if has_background {
            context.set_operator(gtk4::cairo::Operator::Over);
            let current_style = background_style.clone();
            let mut bg_cache = cached_background_surface_draw.borrow_mut();
            let mut bg_style_cache = cached_background_style_draw.borrow_mut();

            if bg_style_cache.as_ref() != Some(&current_style) || bg_cache.is_none() {
                if let BackgroundStyle::Gradient(idx) = &current_style {
                    let surfaces = gradient_surfaces_draw.borrow();
                    if let Some(surface) = surfaces.get(*idx).and_then(|s| s.as_ref()) {
                        *bg_cache = Some(surface.clone());
                    } else {
                        let file_name = background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES[*idx];
                        let path = background_panel::background_gradient_asset_path(file_name);
                        *bg_cache = rgba_image_to_surface(
                            &background_panel::load_background_image_optimized(&path)
                                .unwrap_or_else(|| RgbaImage::new(1, 1)),
                        );
                    }
                } else if let BackgroundStyle::Wallpaper(path) = &current_style {
                    let cache = wallpaper_cache_draw.borrow();
                    if let Some(surface) = cache.get(path) {
                        *bg_cache = Some(surface.clone());
                    } else {
                        println!(
                            "[DEBUG] Cache miss for wallpaper: {:?}, loading synchronously",
                            path
                        );
                        if let Some(rgba) = background_panel::load_background_image_optimized(path)
                        {
                            let surface = rgba_image_to_surface(&rgba);
                            *bg_cache = surface;
                        } else {
                            println!("[DEBUG] Failed to load wallpaper synchronously: {:?}", path);
                            *bg_cache = None;
                        }
                    }
                } else if let BackgroundStyle::PlainColor(_color) = &current_style {
                    *bg_cache = None;
                } else if let BackgroundStyle::Blurred(blur_idx) = &current_style {
                    // Only recompute blur if the working image has changed
                    let current_revision = working_image_revision;
                    let needs_recompute = cached_blurred_revision_draw.get() != current_revision
                        || bg_cache.is_none();

                    if needs_recompute {
                        let mut blurred_bg = working_image.clone();
                        let (bw, bh) = blurred_bg.dimensions();

                        // Optimization: Downsample for background blur to save CPU
                        let max_dim = 800u32;
                        if bw > max_dim || bh > max_dim {
                            let scale = max_dim as f64 / (bw.max(bh) as f64);
                            blurred_bg = image::imageops::resize(
                                &blurred_bg,
                                (bw as f64 * scale) as u32,
                                (bh as f64 * scale) as u32,
                                image::imageops::FilterType::Triangle,
                            );
                        }

                        // Different blur intensities for each tile
                        let blur_radius = match blur_idx {
                            0 => 10.0,  // Light blur
                            1 => 35.0,  // Medium blur
                            2 => 80.0,  // Heavy blur
                            _ => 20.0,  // Default
                        };

                        let (nbw, nbh) = blurred_bg.dimensions();
                        super::render::apply_blur_rect(
                            &mut blurred_bg,
                            Rect {
                                x: 0,
                                y: 0,
                                width: nbw as i32,
                                height: nbh as i32,
                            },
                            blur_radius,
                        );
                        *bg_cache = rgba_image_to_surface(&blurred_bg);
                        cached_blurred_revision_draw.set(current_revision);
                    }
                }
                *bg_style_cache = Some(current_style.clone());
            }

            if let Some(surface) = bg_cache.as_ref() {
                let _ = context.save();
                let sw = surface.width() as f64;
                let sh = surface.height() as f64;
                context.translate(canvas_t.offset_x, canvas_t.offset_y);
                context.scale(
                    (virtual_w * canvas_t.scale) / sw,
                    (virtual_h * canvas_t.scale) / sh,
                );
                context.set_source_surface(surface, 0.0, 0.0).unwrap();
                let _ = context.paint();
                let _ = context.restore();
            } else if let BackgroundStyle::PlainColor(color) = &current_style {
                context.set_source_rgba(color.r, color.g, color.b, color.a);
                context.rectangle(
                    canvas_t.offset_x,
                    canvas_t.offset_y,
                    virtual_w * canvas_t.scale,
                    virtual_h * canvas_t.scale,
                );
                let _ = context.fill();
            }

            let draw_w = image_width * draw_scale_factor;
            let draw_h = image_height * draw_scale_factor;
            let padding_px_scaled = padding_px * canvas_t.scale;

            let (sc_off_x, sc_off_y) = match background_alignment {
                BackgroundAlignment::TopLeft => (padding_px_scaled, padding_px_scaled),
                BackgroundAlignment::TopCenter => (
                    (virtual_w * canvas_t.scale - draw_w * canvas_t.scale) / 2.0,
                    padding_px_scaled,
                ),
                BackgroundAlignment::TopRight => (
                    virtual_w * canvas_t.scale - draw_w * canvas_t.scale - padding_px_scaled,
                    padding_px_scaled,
                ),
                BackgroundAlignment::CenterLeft => (
                    padding_px_scaled,
                    (virtual_h * canvas_t.scale - draw_h * canvas_t.scale) / 2.0,
                ),
                BackgroundAlignment::Center => (
                    (virtual_w * canvas_t.scale - draw_w * canvas_t.scale) / 2.0,
                    (virtual_h * canvas_t.scale - draw_h * canvas_t.scale) / 2.0,
                ),
                BackgroundAlignment::CenterRight => (
                    virtual_w * canvas_t.scale - draw_w * canvas_t.scale - padding_px_scaled,
                    (virtual_h * canvas_t.scale - draw_h * canvas_t.scale) / 2.0,
                ),
                BackgroundAlignment::BottomLeft => (
                    padding_px_scaled,
                    virtual_h * canvas_t.scale - draw_h * canvas_t.scale - padding_px_scaled,
                ),
                BackgroundAlignment::BottomCenter => (
                    (virtual_w * canvas_t.scale - draw_w * canvas_t.scale) / 2.0,
                    virtual_h * canvas_t.scale - draw_h * canvas_t.scale - padding_px_scaled,
                ),
                BackgroundAlignment::BottomRight => (
                    virtual_w * canvas_t.scale - draw_w * canvas_t.scale - padding_px_scaled,
                    virtual_h * canvas_t.scale - draw_h * canvas_t.scale - padding_px_scaled,
                ),
            };

            t.offset_x = canvas_t.offset_x + sc_off_x;
            t.offset_y = canvas_t.offset_y + sc_off_y;
            t.scale = canvas_t.scale * draw_scale_factor;

            if background_shadow > 0.0 {
                let shadow_radius = background_shadow * t.scale * 0.5;
                let shadow_opacity = 0.4;
                let _ = context.save();
                context.set_source_rgba(0.0, 0.0, 0.0, shadow_opacity);
                context.translate(t.offset_x, t.offset_y + shadow_radius * 0.3);

                let rect_w = image_width * t.scale;
                let rect_h = image_height * t.scale;
                let corner_r = background_corner_radius * t.scale;

                context.new_sub_path();
                context.arc(
                    rect_w - corner_r,
                    corner_r,
                    corner_r,
                    -std::f64::consts::FRAC_PI_2,
                    0.0,
                );
                context.arc(
                    rect_w - corner_r,
                    rect_h - corner_r,
                    corner_r,
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                );
                context.arc(
                    corner_r,
                    rect_h - corner_r,
                    corner_r,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                );
                context.arc(
                    corner_r,
                    corner_r,
                    corner_r,
                    std::f64::consts::PI,
                    std::f64::consts::PI * 1.5,
                );
                context.close_path();

                for i in 1..=5 {
                    context.set_line_width(shadow_radius * (i as f64 / 5.0));
                    context.set_source_rgba(0.0, 0.0, 0.0, shadow_opacity / (i as f64));
                    let _ = context.stroke_preserve();
                }
                let _ = context.fill();
                let _ = context.restore();
            }

            let rect_w = image_width * t.scale;
            let rect_h = image_height * t.scale;
            let corner_r = background_corner_radius * t.scale;

            let _ = context.save();
            context.translate(t.offset_x, t.offset_y);
            context.new_sub_path();
            context.arc(
                rect_w - corner_r,
                corner_r,
                corner_r,
                -std::f64::consts::FRAC_PI_2,
                0.0,
            );
            context.arc(
                rect_w - corner_r,
                rect_h - corner_r,
                corner_r,
                0.0,
                std::f64::consts::FRAC_PI_2,
            );
            context.arc(
                corner_r,
                rect_h - corner_r,
                corner_r,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            );
            context.arc(
                corner_r,
                corner_r,
                corner_r,
                std::f64::consts::PI,
                std::f64::consts::PI * 1.5,
            );
            context.close_path();
            context.clip();
            context.translate(-t.offset_x, -t.offset_y);
        }
        context.set_operator(gtk4::cairo::Operator::Over);
        *transform_draw.lock().unwrap() = t;

        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);

        if crop_mode_active && crop_background_color_explicit {
            if let Some(crop_rect) = crop_rect {
                context.set_source_rgba(
                    crop_background_color.r,
                    crop_background_color.g,
                    crop_background_color.b,
                    crop_background_color.a,
                );
                context.rectangle(
                    crop_rect.x as f64,
                    crop_rect.y as f64,
                    crop_rect.width as f64,
                    crop_rect.height as f64,
                );
                let _ = context.fill();
            }
        }

        if cached_surface_revision_draw.get() != working_image_revision
            || cached_surface_draw.borrow().is_none()
        {
            *cached_surface_draw.borrow_mut() = rgba_image_to_surface(&working_image);
            cached_surface_revision_draw.set(working_image_revision);
        }

        if let Some(surface) = cached_surface_draw.borrow().as_ref() {
            super::render::paint_surface_with_filter(
                context,
                surface,
                0.0,
                0.0,
                super::render::editor_image_filter_for_scale(t.scale),
            );
        } else {
            draw_rgba_to_context(context, &working_image);
        }

        for action in &actions {
            if let AnnotationAction::Focus { rect } = action {
                draw_focus_overlay(
                    context,
                    working_image.width() as f64,
                    working_image.height() as f64,
                    *rect,
                    false,
                );
            }
        }

        let editing_action_index = active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index);
        for (index, action) in actions.iter().enumerate() {
            if Some(index) == editing_action_index {
                continue;
            }
            if matches!(
                action,
                AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
            ) {
                continue;
            }
            draw_annotation_action(context, action);
        }

        if let Some(draft) = draft_action {
            if let AnnotationAction::Focus { rect } = &draft {
                draw_focus_overlay(
                    context,
                    working_image.width() as f64,
                    working_image.height() as f64,
                    *rect,
                    true,
                );
            } else {
                draw_draft_action(context, &draft);
            }
        }

        if crop_mode_active {
            if let Some(crop_rect) = crop_rect {
                let canvas_left = -t.offset_x / t.scale;
                let canvas_top = -t.offset_y / t.scale;
                let canvas_width = width as f64 / t.scale;
                let canvas_height = height as f64 / t.scale;
                let _ = context.save();
                context.rectangle(canvas_left, canvas_top, canvas_width, canvas_height);
                context.rectangle(
                    crop_rect.x as f64,
                    crop_rect.y as f64,
                    crop_rect.width as f64,
                    crop_rect.height as f64,
                );
                context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
                context.set_source_rgba(0.0, 0.0, 0.0, 140.0 / 255.0);
                let _ = context.fill();
                let _ = context.restore();
            }
        }

        if let Some(crop_rect) = crop_rect {
            draw_crop_overlay(
                context,
                working_image.width() as f64,
                working_image.height() as f64,
                crop_rect,
                selected_tool == Tool::Crop,
            );
        }

        // In Text tool mode: draw hover outline for the text action under the cursor.
        if selected_tool == Tool::Text && active_text_bounds.is_none() {
            if let Some(hover_idx) = hovered_text_action_index {
                if let Some(action) = actions.get(hover_idx) {
                    if let AnnotationAction::Text {
                        position,
                        text,
                        font,
                        max_width,
                        ..
                    } = action
                    {
                        let available_width = max_width.unwrap_or_else(|| {
                            (working_image.width() as f64 - position.x).max(font.size * 1.8)
                        });
                        let mut text_bounds = text_action_bounds(
                            context,
                            *position,
                            text,
                            font,
                            Some(available_width),
                        );
                        text_bounds.rect.x = text_bounds.rect.x.clamp(
                            0,
                            (working_image.width() as i32 - text_bounds.rect.width).max(0),
                        );
                        text_bounds.rect.y = text_bounds.rect.y.clamp(
                            0,
                            (working_image.height() as i32 - text_bounds.rect.height).max(0),
                        );
                        text_bounds.sync_handles();
                        draw_text_edit_border(context, &text_bounds, t.scale);
                    }
                }
            }
        }

        if let Some(selected_action) = selected_action.as_ref() {
            if selected_tool == Tool::Select
                && select_drag_anchor.is_some()
                && matches!(selected_action, AnnotationAction::Obfuscate { .. })
            {
                draw_draft_action(context, selected_action);
            }

            // Draw border + handles for a selected Text action in both
            // Select tool mode and Text tool mode (e.g. during drag-to-move).
            let show_text_handles = (selected_tool == Tool::Select || selected_tool == Tool::Text)
                && active_text_bounds.is_none();

            if show_text_handles {
                if let AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                } = selected_action
                {
                    let available_width = max_width.unwrap_or_else(|| {
                        (working_image.width() as f64 - position.x).max(font.size * 1.8)
                    });
                    let mut text_bounds =
                        text_action_bounds(context, *position, text, font, Some(available_width));
                    text_bounds.rect.x = text_bounds.rect.x.clamp(
                        0,
                        (working_image.width() as i32 - text_bounds.rect.width).max(0),
                    );
                    text_bounds.rect.y = text_bounds.rect.y.clamp(
                        0,
                        (working_image.height() as i32 - text_bounds.rect.height).max(0),
                    );
                    text_bounds.sync_handles();
                    draw_text_edit_border(context, &text_bounds, t.scale);
                    draw_text_edit_handles(context, &text_bounds, None, t.scale);
                }
            }

            if selected_tool == Tool::Select || selected_tool == Tool::Arrow {
                if let AnnotationAction::Text { .. } = selected_action {
                    // Already handled above.
                } else if let AnnotationAction::Arrow {
                    start,
                    end,
                    stroke_size,
                    style,
                    control_points,
                    ..
                } = selected_action
                {
                    draw_arrow_selection_outline(
                        context,
                        *start,
                        *end,
                        *stroke_size,
                        *style,
                        control_points.clone(),
                        t.scale,
                    );
                } else if matches!(selected_action, AnnotationAction::Line { .. }) {
                    // Intentionally show no crop-like selection outline or handles for lines.
                } else {
                    let selection_padding = selection_hit_padding_for_scale(t.scale);
                    if let Some(bounds) =
                        action_bounds_with_padding(selected_action, selection_padding)
                    {
                        draw_selection_outline(context, bounds, t.scale);
                    }

                    let handles = action_resize_handles(selected_action);
                    if !handles.is_empty() {
                        draw_selection_handles(context, &handles, select_resize_handle, t.scale);
                    }
                }
            }

            // The active text edit overlay (border + handles) is drawn by the
            // unconditional block below, which also handles clamping and cursor
            // rendering. Do NOT draw it here a second time.
        }

        // Draw arrow control handles when: (a) editing controls are active, OR
        // (b) Arrow or Select tool is selected and an existing arrow is selected.
        let show_handles = arrow_editing_controls
            || ((selected_tool == Tool::Arrow || selected_tool == Tool::Select)
                && selected_action
                    .as_ref()
                    .map(|a| matches!(a, AnnotationAction::Arrow { .. }))
                    .unwrap_or(false));

        if show_handles {
            if let Some(action) = selected_action.as_ref() {
                if let AnnotationAction::Arrow {
                    control_points: Some(handles),
                    color,
                    ..
                } = action
                {
                    draw_arrow_control_handles(context, handles.clone(), *color, t.scale);
                }
            }
        }

        // Draw active text edit overlay (border + handles)
        if let Some(bounds) = active_text_bounds.as_ref() {
            let mut bounds = bounds.clone();
            bounds.rect.x = bounds
                .rect
                .x
                .clamp(0, (working_image.width() as i32 - bounds.rect.width).max(0));
            bounds.rect.y = bounds.rect.y.clamp(
                0,
                (working_image.height() as i32 - bounds.rect.height).max(0),
            );
            bounds.sync_handles();
            if let Some(input) = active_text_input.as_ref() {
                let font = super::types::FontSettings {
                    family: text_font_family.clone(),
                    size: text_size,
                    style: super::types::FontStyle::Normal,
                    decoration: super::types::TextDecoration::None,
                    alignment: super::types::TextAlignment::Left,
                };
                draw_active_text_input(
                    context,
                    &bounds,
                    &input.text,
                    input.cursor_position,
                    input.cursor_visible,
                    input.color,
                    &font,
                );
            }
            draw_text_edit_border(context, &bounds, t.scale);
            draw_text_edit_handles(context, &bounds, active_text_drag_handle.clone(), t.scale);
        }
        let _ = context.restore();
    });

    let tool_buttons = vec![
        crop_btn.clone(),
        background_btn.clone(),
        select_btn.clone(),
        draw_btn.clone(),
        box_btn.clone(),
        circle_btn.clone(),
        arrow_btn.clone(),
        line_btn.clone(),
        text_btn.clone(),
        obfuscate_btn.clone(),
        number_btn.clone(),
        highlighter_btn.clone(),
        focus_btn.clone(),
    ];

    // Set initial active tool button (Background is default)
    background_btn.add_css_class("active-tool");

    events::wire_editor_events(events::EventContext {
        app: app.clone(),
        window: window.clone(),
        path: path.clone(),
        state: state.clone(),
        transform: transform.clone(),
        drawing_area: drawing_area.clone(),
        tool_buttons: tool_buttons.clone(),
        select_btn: select_btn.clone(),
        crop_btn: crop_btn.clone(),
        background_btn: background_btn.clone(),
        draw_btn: draw_btn.clone(),
        arrow_btn: arrow_btn.clone(),
        line_btn: line_btn.clone(),
        box_btn: box_btn.clone(),
        circle_btn: circle_btn.clone(),
        text_btn: text_btn.clone(),
        number_btn: number_btn.clone(),
        highlighter_btn: highlighter_btn.clone(),
        obfuscate_btn: obfuscate_btn.clone(),
        focus_btn: focus_btn.clone(),
        traffic_close: traffic_close.clone(),
        traffic_minimize: traffic_minimize.clone(),
        traffic_zoom: traffic_zoom.clone(),
        canvas_overlay: canvas_overlay.clone(),
        canvas_scroller: canvas_scroller.clone(),
        zoom_button: zoom_button.clone(),
        zoom_label: zoom_label.clone(),
        zoom_header_label: zoom_header_label.clone(),
        zoom_popup: zoom_popup.clone(),
        zoom_minus_btn: zoom_minus_btn.clone(),
        zoom_plus_btn: zoom_plus_btn.clone(),
        zoom_in_btn: zoom_in_btn.clone(),
        zoom_out_btn: zoom_out_btn.clone(),
        fit_to_screen_btn: fit_to_screen_btn.clone(),
        zoom_to_selection_btn: zoom_to_selection_btn.clone(),
        zoom_level: zoom_level.clone(),
        copy_btn: copy_btn.clone(),
        upload_btn: upload_btn.clone(),
        color_buttons: color_buttons.clone(),
        color_picker_dot: color_picker_dot.clone(),
        color_class_names: color_class_names.clone(),
        color_popover: color_popover.clone(),
        size_slider: size_slider.clone(),
        text_size_label: text_size_label.clone(),
        font_family_label: font_family_label.clone(),
        text_size_list: text_size_list.clone(),
        font_family_list: font_family_list.clone(),
        apply_crop_btn: crop_apply_btn.clone(),
        crop_reset_btn: crop_reset_btn.clone(),
        undo_btn: undo_btn.clone(),
        redo_btn: redo_btn.clone(),
        delete_selected_btn: delete_selected_btn.clone(),
        save_btn: save_btn.clone(),
        eyedropper_mode: eyedropper_mode.clone(),
        eyedropper_from_sidebar: eyedropper_from_sidebar.clone(),
        eyedropper_point: eyedropper_point.clone(),
        eyedropper_rendered: eyedropper_rendered.clone(),
        canvas_eyedropper_ring: canvas_eyedropper_ring.clone(),
        update_toolbar_for_tool: update_toolbar_for_tool.clone(),
        update_crop_size_fields: update_crop_size_fields.clone(),
        update_canvas_content_size: update_canvas_content_size.clone(),
        sync_picker_for_active_tool: sync_shared_colors_for_active_tool.clone(),
        sync_picker_from_color: sync_picker_from_color.clone(),
        apply_picker_color_to_editor: apply_picker_color_to_editor.clone(),
        add_color_to_custom_slots: Rc::new({
            let custom_slot_colors = custom_slot_colors.clone();
            let refresh_custom_color_slots = refresh_custom_color_slots.clone();
            let refresh_colors_panel_custom_slots = refresh_colors_panel_custom_slots.clone();
            move |color: DrawColor| {
                let mut custom_colors = custom_slot_colors.borrow_mut();
                if let Some(slot_index) = custom_colors.iter().position(Option::is_none) {
                    custom_colors[slot_index] = Some(color);
                    super::color::save_persisted_custom_slot_colors(custom_colors.as_slice());
                    drop(custom_colors);
                    refresh_custom_color_slots();
                    refresh_colors_panel_custom_slots();
                }
            }
        }),
        set_picker_panel_visibility: set_picker_panel_visibility.clone(),
        sync_size_control: sync_size_control.clone(),
        rebuild_effects_async: rebuild_effects_async.clone(),
        obfuscate_method_button: obfuscate_method_button.clone(),
        obfuscate_method_list: obfuscate_method_list.clone(),
        pen_weight_button: pen_weight_button.clone(),
        pen_weight_list: pen_inspector_list.clone(),
        highlighter_weight_list: highlighter_inspector_list.clone(),
        number_options_list: number_options_list.clone(),
        number_start_entry: number_start_entry.clone(),
        number_inc_btn: number_inc_btn.clone(),
        number_dec_btn: number_dec_btn.clone(),
        number_size_button: number_size_button.clone(),
        number_size_list: number_size_list.clone(),
        arrow_style_button: arrow_style_button.clone(),
        arrow_style_list: arrow_style_list.clone(),
        arrow_thickness_list: arrow_thickness_list.clone(),
        inverse_direction_toggle: inverse_direction_toggle.clone(),
        stroke_size_button: stroke_size_button.clone(),
        stroke_size_list: line_inspector_list.clone(),
    });

    window.present();
    set_window_dock_visibility(&window, annotate_config.show_dock_icon);
    if annotate_config.always_on_top {
        set_window_always_on_top(
            &window,
            &tracked_window_id,
            true,
            window_title,
            window_namespace,
        );
    }

    window.connect_close_request(move |_| {
        crate::gnome_integration::emit_tracked_window_closed(&tracked_window_id);
        glib::Propagation::Proceed
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn toolbar_color_status_syncs_from_shared_active_color() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-toolbar-color-status-swatch")
                && production_source.contains("color_status_label.set_label")
                && production_source.contains("BackgroundStyle::PlainColor")
                && production_source.contains("draw_color_to_hex(active_color)"),
            "Toolbar color status should mirror the shared active color, including Background plain color",
        );
    }

    #[test]
    fn toolbar_color_status_follows_colors_panel_palette_and_custom_color_updates() {
        let mod_source = include_str!("mod.rs");
        let mod_production_source = mod_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(mod_source);
        let colors_source = include_str!("colors_panel.rs");
        let colors_production_source = colors_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(colors_source);
        assert!(
            mod_production_source.contains("sync_toolbar_color_status();")
                && colors_production_source.contains("apply_picker_color(DRAW_COLORS[index]);")
                && colors_production_source.contains("apply_picker_color_click(color);"),
            "Toolbar color status should follow palette and My colors selections from the Colors panel",
        );
    }

    #[test]
    fn editor_layout_includes_persistent_right_inspector_shell() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(production_source.contains("editor-right-inspector"));
    }

    #[test]
    fn editor_layout_tracks_background_inspector_content() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(production_source.contains("background_inspector"));
    }

    #[test]
    fn editor_layout_uses_stack_for_inspector_surface_switching() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let inspector_stack = Stack::new();")
                && production_source.contains("inspector_stack.set_hhomogeneous(true);")
                && production_source.contains("background_inspector.set_visible(true);")
                && production_source.contains("crop_inspector.set_visible(true);")
                && production_source.contains("pen_inspector.set_visible(true);")
                && production_source.contains("arrow_inspector.set_visible(true);")
                && production_source.contains("line_inspector.set_visible(true);")
                && production_source.contains("text_inspector.set_visible(true);")
                && production_source.contains("highlighter_inspector.set_visible(true);")
                && production_source.contains("number_inspector.set_visible(true);")
                && production_source.contains("colors_inspector.set_visible(true);")
                && production_source.contains("placeholder_inspector.set_visible(true);")
                && production_source.contains("inspector_stack.add_named(&background_inspector, Some(\"background\"));")
                && production_source.contains("inspector_stack.add_named(&crop_inspector, Some(\"crop\"));")
                && production_source.contains("inspector_stack.add_named(&pen_inspector, Some(\"pen\"));")
                && production_source.contains("inspector_stack.add_named(&arrow_inspector, Some(\"arrow\"));")
                && production_source.contains("inspector_stack.add_named(&line_inspector, Some(\"line\"));")
                && production_source.contains("inspector_stack.add_named(&text_inspector, Some(\"text\"));")
                && production_source.contains("inspector_stack.add_named(&highlighter_inspector, Some(\"highlighter\"));")
                && production_source.contains("inspector_stack.add_named(&number_inspector, Some(\"number\"));")
                && production_source.contains("inspector_stack.add_named(&colors_inspector, Some(\"colors\"));")
                && production_source.contains("inspector_stack.add_named(&placeholder_inspector, Some(\"placeholder\"));")
                && production_source.contains("inspector_stack.set_visible_child_name(surface);"),
            "Inspector surfaces should switch through a dedicated stack so tab changes keep one stable container",
        );
    }

    #[test]
    fn editor_startup_uses_selected_tool_instead_of_hardcoded_arrow() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let initial_tool = state.lock().unwrap().selected_tool;")
                && production_source.contains("update_toolbar_for_tool(initial_tool);")
                && !production_source.contains("update_toolbar_for_tool(Tool::Arrow);"),
            "Editor startup should route the inspector from the selected startup tool instead of forcing Arrow",
        );
    }

    #[test]
    fn crop_pen_arrow_line_text_number_and_highlighter_route_to_tool_specific_inspector_tabs() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("Tool::Crop")
                && production_source.contains("Tool::Pen")
                && production_source.contains("\"crop\"")
                && production_source.contains("\"pen\"")
                && production_source.contains("Tool::Arrow")
                && production_source.contains("Tool::Line")
                && production_source.contains("Tool::Text")
                && production_source.contains("Tool::Number")
                && production_source.contains("Tool::Highlighter")
                && production_source.contains("\"arrow\"")
                && production_source.contains("\"line\"")
                && production_source.contains("\"text\"")
                && production_source.contains("\"number\"")
                && production_source.contains("\"highlighter\"")
                && production_source.contains("\"colors\""),
            "Inspector routing should expose Pen, Arrow, Line, Text, Number, and Highlighter primary panels alongside the shared Colors surface",
        );
    }

    #[test]
    fn crop_inspector_includes_aspect_ratio_dimensions_and_actions_sections() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source
                .contains("let (crop_inspector, crop_inspector_content) = build_tool_inspector();")
                && production_source.contains("\"Aspect Ratio\"")
                && production_source.contains("\"Dimensions\"")
                && production_source.contains("\"Actions\""),
            "Crop inspector should render Aspect Ratio, Dimensions, and Actions sections",
        );
    }

    #[test]
    fn crop_inspector_reuses_existing_fixed_sidebar_width() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("CROP_SIDEBAR_WIDTH"),
            "Crop inspector should reuse the shared fixed sidebar width instead of introducing a new width path",
        );
    }

    #[test]
    fn crop_dimensions_use_active_crop_rect_in_the_inspector() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("st.draft_crop_rect().or(st.crop_selection)")
                && production_source.contains("crop_width_value.set_label")
                && production_source.contains("crop_height_value.set_label"),
            "Crop dimensions should mirror the active draft or committed crop rect in the side inspector",
        );
    }

    #[test]
    fn arrow_inspector_includes_style_thickness_and_behavior_sections() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(
                "let (arrow_inspector, arrow_inspector_content) = build_tool_inspector();"
            ) && production_source.contains("\"Style\"")
                && production_source.contains("\"Thickness\"")
                && production_source.contains("\"Behavior\""),
            "Arrow inspector should render Style, Thickness, and Behavior sections",
        );
    }

    #[test]
    fn arrow_inspector_reuses_existing_fixed_sidebar_width() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("ARROW_SIDEBAR_WIDTH"),
            "Arrow inspector should reuse the shared fixed sidebar width instead of introducing a new width path",
        );
    }

    #[test]
    fn arrow_thickness_options_use_custom_stroke_previews() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("fn build_arrow_thickness_preview(weight: super::pen_weight::PenWeight) -> DrawingArea")
                && production_source.contains("let icon = build_arrow_thickness_preview(weight);")
                && !production_source.contains("let icon = Image::from_icon_name(weight.icon_name());\n        icon.set_pixel_size(weight.icon_pixel_size());\n        let label_widget = Label::new(Some(label));"),
            "Arrow thickness inspector options should use dedicated stroke previews instead of stock symbolic icons",
        );
    }

    #[test]
    fn arrow_inspector_style_and_thickness_rows_include_tick_indicators() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("check_icon.add_css_class(\"editor-arrow-inspector-check\");")
                && production_source.contains("btn.add_css_class(\"editor-arrow-inspector-option-active\");")
                && production_source.contains("sync_arrow_option_selection(&arrow_style_list, selected_style);")
                && production_source.contains("sync_arrow_option_selection(&arrow_thickness_list, selected_thickness);"),
            "Arrow inspector rows should expose a visible selected tick for style and thickness options",
        );
    }

    #[test]
    fn pen_line_and_highlighter_inspectors_use_thickness_sections_with_arrow_row_styles() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let (pen_inspector, pen_inspector_content) = build_tool_inspector();")
                && production_source.contains("let (line_inspector, line_inspector_content) = build_tool_inspector();")
                && production_source.contains("let (highlighter_inspector, highlighter_inspector_content) = build_tool_inspector();")
                && production_source.contains("&pen_inspector_content")
                && production_source.contains("&line_inspector_content")
                && production_source.contains("&highlighter_inspector_content")
                && production_source.contains("\"Thickness\"")
                && production_source.contains("pen_inspector_list.upcast_ref()")
                && production_source.contains("line_inspector_list.upcast_ref()")
                && production_source.contains("highlighter_inspector_list.upcast_ref()")
                && production_source.contains("\"editor-arrow-inspector-option\"")
                && production_source.contains("sync_arrow_option_selection(&pen_inspector_list, selected_pen_thickness);")
                && production_source.contains("sync_arrow_option_selection(&line_inspector_list, selected_thickness);")
                && production_source.contains("sync_arrow_option_selection(&highlighter_inspector_list, selected_pen_thickness);"),
            "Pen, Line, and Highlighter inspectors should expose thickness sections using the same active row styling as the Arrow inspector",
        );
    }

    #[test]
    fn text_inspector_rows_use_label_plus_tick_layout_and_shared_sidebar_width() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("check_icon.add_css_class(\"editor-text-inspector-check\");")
                && production_source.contains("btn.add_css_class(\"editor-text-inspector-option-active\");")
                && production_source.contains("sync_text_option_selection(&text_size_list")
                && production_source.contains("sync_text_option_selection(&font_family_list")
                && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("TEXT_SIDEBAR_WIDTH"),
            "Text inspector rows should use explicit selected ticks while reusing the existing fixed sidebar width",
        );
    }

    #[test]
    fn obfuscate_inspector_renders_method_section_and_reuses_shared_sidebar_width() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains(
                "let (obfuscate_inspector, obfuscate_inspector_content) = build_tool_inspector();"
            ) && production_source.contains("\"Method\"")
                && production_source
                    .contains("sync_obfuscate_option_selection(&obfuscate_method_list")
                && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("OBFUSCATE_SIDEBAR_WIDTH"),
            "Obfuscate should render a Method section while reusing the shared fixed sidebar width",
        );
    }

    #[test]
    fn obfuscate_inspector_uses_a_fresh_list_instead_of_reusing_toolbar_popover_state() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let obfuscate_method_list = GtkBox::new(Orientation::Vertical, 0);")
                && !production_source.contains("if let Some(parent) = obfuscate_method_list.parent() {"),
            "Obfuscate should follow the migrated tool pattern and build a fresh inspector-owned list instead of reusing toolbar popover state",
        );
    }

    #[test]
    fn number_inspector_style_and_size_rows_use_matching_row_composition() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("editor-number-style-check")
                && production_source.contains("editor-number-size-check")
                && production_source.contains("editor-number-style-option-active")
                && production_source.contains("editor-number-size-option-active")
                && production_source.contains("sync_number_option_selection(")
                && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("NUMBER_SIDEBAR_WIDTH"),
            "Number Style and Size rows should share the same inspector-native composition while reusing the shared sidebar width",
        );
    }

    #[test]
    fn number_inspector_includes_start_controls_in_the_sidebar() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let number_start_row = GtkBox::new(Orientation::Horizontal, 8);")
                && production_source.contains("let number_start_label = Label::new(Some(\"Start with:\"));")
                && production_source.contains("number_start_row.append(&number_dec_btn);")
                && production_source.contains("number_start_row.append(&number_start_entry);")
                && production_source.contains("number_start_row.append(&number_inc_btn);")
                && production_source.contains("append_inspector_section(&number_inspector_content, \"Start\", number_start_row.upcast_ref());"),
            "Number inspector should expose the starting number controls inside the sidebar",
        );
    }
}
