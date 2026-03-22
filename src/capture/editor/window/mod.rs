use gtk4::{
    glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, Orientation, Popover,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::color::selection_hit_padding_for_scale;
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
    AnnotationAction, BackgroundAlignment, BackgroundStyle, CropAspectRatio, EditorError, Point,
    Rect, Tool, ViewTransform,
};
use super::ui_support::{
    install_editor_css, prefers_dark_glass_theme, prefers_reduced_transparency,
    recommended_window_size,
};

pub mod background_panel;
mod canvas;
pub mod color_picker;
#[allow(dead_code)]
mod cursor;
mod events;
mod footer;
mod toolbar;

mod icon_names {
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

#[cfg(test)]
pub(super) use cursor::cursor_name_for_view_point;

pub fn setup_editor_window(app: &Application, path: PathBuf) {
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    install_editor_css();

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

    let (default_width, default_height) =
        recommended_window_size(img_width as i32, img_height as i32);

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
        draw: icon_names::DOCUMENT_EDIT_REGULAR,
        arrow: icon_names::GO_NEXT,
        line: icon_names::DRAW_LINE,
        box_: icon_names::DRAW_RECTANGLE,
        circle: icon_names::CIRCLE_LINE_REGULAR,
        text: icon_names::TEXT_FONT_REGULAR,
        number: icon_names::NUMBER_CIRCLE_1_REGULAR,
        highlighter: icon_names::HIGHLIGHT_REGULAR,
        obfuscate: icon_names::FOG,
        focus: icon_names::SMALL_RECTANGLE_IN_FOCUS,
        obfuscate_pixelate: "view-grid-symbolic",
        obfuscate_blur_secure: "security-high-symbolic",
        obfuscate_blur_smooth: "blur-symbolic",
        obfuscate_blackout: "media-playback-stop-symbolic",
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
    );
    let color_picker_trigger_host = color_picker_parts.trigger_host;
    let color_popover = color_picker_parts.popover;
    let color_buttons = color_picker_parts.color_buttons;
    let color_picker_dot = color_picker_parts.color_picker_dot;
    let color_class_names = color_picker_parts.color_class_names;
    let eyedropper_btn = color_picker_parts.eyedropper_btn;
    let sync_picker_for_active_tool = color_picker_parts.sync_for_active_tool;
    let sync_picker_from_color = color_picker_parts.sync_picker_from_color;
    let apply_picker_color_to_editor = color_picker_parts.apply_picker_color;
    let set_picker_panel_visibility = color_picker_parts.set_picker_panel_visibility;

    let toolbar::ToolbarModeParts {
        root: center_group,
        toolbar_mode_stack,
        size_group,
        size_slider,
        text_size_group,
        text_size_label,
        text_size_list,
        font_family_group,
        font_family_label,
        font_family_list,
        crop_type_label,
        crop_type_popover,
        crop_type_list,
        crop_width_entry,
        crop_height_entry,
        obfuscate_method_group,
        obfuscate_method_button,
        obfuscate_method_popover: _,
        obfuscate_method_list,
        pen_weight_button,
        pen_weight_popover: _,
        pen_weight_list,
        pen_weight_group,
        number_options_popover: _,
        number_options_list,
        number_start_entry,
        number_inc_btn,
        number_dec_btn,
        number_size_button,
        number_size_popover: _,
        number_size_list,
        number_options_group,
        arrow_style_group,
        arrow_style_button,
        arrow_style_popover: _,
        arrow_style_list,
        stroke_size_group,
        stroke_size_button,
        stroke_size_popover: _,
        stroke_size_list,
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
        &color_picker_trigger_host,
    );
    toolbar.set_center_widget(Some(&center_group));

    let toolbar_right_parts = toolbar::build_toolbar_right_controls(
        icon_names::ARROW_UNDO_REGULAR,
        icon_names::ARROW_REDO_REGULAR,
    );
    let undo_btn = toolbar_right_parts.undo_btn;
    let redo_btn = toolbar_right_parts.redo_btn;
    let delete_selected_btn = toolbar_right_parts.delete_selected_btn;
    let save_btn = toolbar_right_parts.save_btn;
    let apply_crop_btn = toolbar_right_parts.apply_crop_btn;
    toolbar.set_end_widget(Some(&toolbar_right_parts.root));

    let footer_parts = footer::build_footer(
        icon_names::VIEW_PIN,
        icon_names::COPY_REGULAR,
        icon_names::CLOUD_ARROW_UP_REGULAR,
    );
    let pin_btn = footer_parts.pin_btn;
    let pin_icon = footer_parts.pin_icon;
    let drag_btn = footer_parts.drag_btn;
    let copy_btn = footer_parts.copy_btn;
    let upload_btn = footer_parts.upload_btn;

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
    let background_sidebar = background_panel_parts.sidebar;
    let start_background_gradient_preview_loading =
        background_panel_parts.start_gradient_preview_loading;

    // Re-parent sidebar into canvas workspace
    if let Some(canvas_workspace) = canvas
        .first_child()
        .and_then(|c| c.downcast::<GtkBox>().ok())
    {
        if let Some(placeholder) = canvas_workspace.first_child() {
            canvas_workspace.remove(&placeholder);
        }
        canvas_workspace.prepend(&background_sidebar);
    }
    *drawing_area_placeholder.borrow_mut() = Some(drawing_area.downgrade());

    let update_toolbar_for_tool = toolbar::build_toolbar_tool_updater(
        &toolbar_mode_stack,
        &background_sidebar,
        &text_size_group,
        &font_family_group,
        &obfuscate_method_group,
        &pen_weight_group,
        &number_options_group,
        &arrow_style_group,
        &stroke_size_group,
        &canvas_scroller,
        start_background_gradient_preview_loading.clone(),
        &window,
        img_width as i32,
        img_height as i32,
    );

    let canvas_padding = canvas::CANVAS_PADDING;

    let update_crop_size_fields: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let crop_width_entry = crop_width_entry.clone();
        let crop_height_entry = crop_height_entry.clone();
        move || {
            let st = state.lock().unwrap();
            if let Some(rect) = st.draft_crop_rect().or(st.crop_selection) {
                crop_width_entry.set_text(&rect.width.max(0).to_string());
                crop_height_entry.set_text(&rect.height.max(0).to_string());
            } else {
                crop_width_entry.set_text("");
                crop_height_entry.set_text("");
            }
        }
    });

    for crop_type in CropAspectRatio::ALL {
        let option_button = Button::with_label(crop_type.label());
        option_button.set_has_frame(false);
        option_button.add_css_class("editor-crop-type-option");
        let crop_type_label_option = crop_type_label.clone();
        let crop_type_popover_option = crop_type_popover.clone();
        let state_crop_type_option = state.clone();
        let drawing_area_crop_type_option = drawing_area.downgrade();
        let update_crop_size_fields_option = update_crop_size_fields.clone();
        option_button.connect_clicked(move |_| {
            crop_type_label_option.set_label(crop_type.label());
            {
                let mut st = state_crop_type_option.lock().unwrap();
                st.set_crop_aspect_ratio(crop_type);
                if st.selected_tool == Tool::Crop {
                    st.ensure_crop_selection_initialized();
                }
            }
            update_crop_size_fields_option();
            crop_type_popover_option.popdown();
            if let Some(area) = drawing_area_crop_type_option.upgrade() {
                area.queue_draw();
            }
        });
        crop_type_list.append(&option_button);
    }

    while let Some(child) = text_size_list.first_child() {
        text_size_list.remove(&child);
    }
    for size in [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72] {
        let label = format!("{}pt", size);
        let btn = Button::builder()
            .label(&label)
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .build();
        let state = state.clone();
        let text_size_label = text_size_label.clone();
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
    for family in ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"] {
        let btn = Button::builder()
            .label(family)
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat"])
            .build();
        let state = state.clone();
        let font_family_label = font_family_label.clone();
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
            if has_active_text {
                drawing_area.grab_focus();
            }
            drawing_area.queue_draw();
        });
        font_family_list.append(&btn);
    }

    let eyedropper_mode = Rc::new(Cell::new(false));
    let eyedropper_point = Rc::new(RefCell::new(None::<Point>));
    let eyedropper_rendered = Rc::new(RefCell::new(None::<RgbaImage>));

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
    root.append(&canvas);
    root.append(&footer_parts.root);
    window.set_child(Some(&root));

    let update_canvas_content_size: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        let canvas_overlay = canvas_overlay.clone();
        let canvas_scroller = canvas_scroller.clone();
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
            let available_width = (scroller_width - (canvas_padding * 2 + 2) as f64).max(1.0);
            let scale = (available_width / virtual_w).min(1.0);
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
            drawing_area.set_size_request(canvas_w, canvas_h);
            canvas_overlay.set_size_request(canvas_w, canvas_h);
        }
    });
    update_canvas_content_size();

    {
        let update_canvas_content_size_tick = update_canvas_content_size.clone();
        let state_canvas_tick = state.clone();
        let last_canvas_signature = Rc::new(Cell::new((
            0_i32, 0_i32, 0_i32, 0_i32, 0_i32, 0_i32, 0_i32, false,
        )));
        let last_canvas_signature_tick = last_canvas_signature.clone();
        canvas_scroller.add_tick_callback(move |scroller, _| {
            let width = scroller.allocated_width();
            let signature = {
                let st = state_canvas_tick.lock().unwrap();
                let crop_rect = st.draft_crop_rect().or(st.crop_selection);
                let (crop_x, crop_y, crop_w, crop_h) = crop_rect
                    .map(|rect| (rect.x, rect.y, rect.width, rect.height))
                    .unwrap_or((0, 0, 0, 0));
                (
                    width,
                    st.working_image.width().max(1) as i32,
                    st.working_image.height().max(1) as i32,
                    crop_x,
                    crop_y,
                    crop_w,
                    crop_h,
                    st.selected_tool == Tool::Crop,
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
        move || {
            // Extract all needed data BEFORE any GTK operations to avoid deadlock
            let (selected_tool, mode, value, text_size, font_family) = {
                let st = state.lock().unwrap();
                (
                    st.selected_tool,
                    st.active_size_control_mode(),
                    st.active_size_value().unwrap_or_default(),
                    st.text_size,
                    st.text_font_family.clone(),
                )
            };

            text_size_label.set_label(&format!("{}pt", text_size as i32));
            font_family_label.set_label(&font_family);

            // Now perform GTK operations WITHOUT holding the lock
            if selected_tool == Tool::Highlighter {
                size_group.set_visible(false);
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

    let state_draw = state.clone();
    let transform_draw = transform.clone();
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

        let mut t = ViewTransform::fit(virtual_w, virtual_h, view_width, view_height);
        t.offset_x += canvas_padding_draw + overflow_left;
        t.offset_y += canvas_padding_draw + overflow_top;

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
                } else if let BackgroundStyle::Blurred(_idx) = &current_style {
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

                        let (nbw, nbh) = blurred_bg.dimensions();
                        super::render::apply_blur_rect(
                            &mut blurred_bg,
                            Rect {
                                x: 0,
                                y: 0,
                                width: nbw as i32,
                                height: nbh as i32,
                            },
                            20.0,
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
            if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
                let _ = context.paint();
            }
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

            if selected_tool == Tool::Select {
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

        // Draw arrow control handles for Curved/Double editing
        if arrow_editing_controls {
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
        pin_btn: pin_btn.clone(),
        pin_icon: pin_icon.clone(),
        drag_btn: drag_btn.clone(),
        copy_btn: copy_btn.clone(),
        upload_btn: upload_btn.clone(),
        color_buttons: color_buttons.clone(),
        color_picker_dot: color_picker_dot.clone(),
        color_class_names: color_class_names.clone(),
        color_popover: color_popover.clone(),
        size_slider: size_slider.clone(),
        text_size_label: text_size_label.clone(),
        font_family_label: font_family_label.clone(),
        apply_crop_btn: apply_crop_btn.clone(),
        undo_btn: undo_btn.clone(),
        redo_btn: redo_btn.clone(),
        delete_selected_btn: delete_selected_btn.clone(),
        save_btn: save_btn.clone(),
        eyedropper_mode: eyedropper_mode.clone(),
        eyedropper_point: eyedropper_point.clone(),
        eyedropper_rendered: eyedropper_rendered.clone(),
        canvas_eyedropper_ring: canvas_eyedropper_ring.clone(),
        update_toolbar_for_tool: update_toolbar_for_tool.clone(),
        update_crop_size_fields: update_crop_size_fields.clone(),
        update_canvas_content_size: update_canvas_content_size.clone(),
        sync_picker_for_active_tool: sync_picker_for_active_tool.clone(),
        sync_picker_from_color: sync_picker_from_color.clone(),
        apply_picker_color_to_editor: apply_picker_color_to_editor.clone(),
        set_picker_panel_visibility: set_picker_panel_visibility.clone(),
        sync_size_control: sync_size_control.clone(),
        rebuild_effects_async: rebuild_effects_async.clone(),
        obfuscate_method_button: obfuscate_method_button.clone(),
        obfuscate_method_list: obfuscate_method_list.clone(),
        pen_weight_button: pen_weight_button.clone(),
        pen_weight_list: pen_weight_list.clone(),
        number_options_list: number_options_list.clone(),
        number_start_entry: number_start_entry.clone(),
        number_inc_btn: number_inc_btn.clone(),
        number_dec_btn: number_dec_btn.clone(),
        number_size_button: number_size_button.clone(),
        number_size_list: number_size_list.clone(),
        arrow_style_button: arrow_style_button.clone(),
        arrow_style_list: arrow_style_list.clone(),
        stroke_size_button: stroke_size_button.clone(),
        stroke_size_list: stroke_size_list.clone(),
    });

    window.present();
}
