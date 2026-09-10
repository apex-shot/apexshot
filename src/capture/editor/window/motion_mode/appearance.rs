use gtk4::cairo::Context;
use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, FileChooserAction,
    FileChooserNative, FileFilter, Label, Orientation, Overlay, ResponseType, Stack,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::MotionBackgroundFillType;

use super::widgets::{
    motion_appearance_slider, motion_color_control, motion_gradient_color_control, motion_rgba,
};
use super::MotionSession;

/// Shotbase keeps Motion appearance as a scene-level inspector rather than an
/// animation clip.  The five fill controls map one-to-one to the recovered
/// `BackgroundFillType` cases and only mutate the compositor state.
pub(super) fn build_motion_appearance_panel(
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
) -> GtkBox {
    let (
        initial_background_color,
        initial_gradient_start,
        initial_gradient_end,
        initial_border,
        initial_shadow_opacity,
        initial_shadow_blur,
        initial_shadow_position,
        initial_fill_type,
    ) = {
        let runtime = session.runtime.borrow();
        let appearance = &runtime.motion.appearance;
        (
            motion_rgba(appearance.background_color),
            motion_rgba(appearance.gradient_color_1),
            motion_rgba(appearance.gradient_color_2),
            motion_rgba(appearance.border_fill_color),
            appearance.shadow_opacity,
            appearance.shadow_blur,
            appearance.shadow_position,
            appearance.background_fill_type.clone(),
        )
    };
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.add_css_class("editor-inspector-placeholder-shell");
    root.add_css_class("editor-motion-inspector");
    root.set_hexpand(false);
    root.set_vexpand(false);

    let title = Label::new(Some(&t("Appearance")));
    title.add_css_class("editor-inspector-title");
    title.set_xalign(0.0);
    root.append(&title);

    let fill_section = GtkBox::new(Orientation::Vertical, 0);
    fill_section.add_css_class("editor-motion-background-picker");
    let background_section = motion_appearance_section("Background");
    let none_button = Button::with_label(&t("None"));
    none_button.set_has_frame(false);
    none_button.set_hexpand(true);
    none_button.add_css_class("editor-background-option-button");
    if initial_fill_type == MotionBackgroundFillType::None {
        none_button.add_css_class("active-background-option");
    }
    none_button.connect_clicked({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        move |_| {
            runtime.borrow_mut().motion.appearance.background_fill_type =
                MotionBackgroundFillType::None;
            none_button.add_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    fill_section.append(&none_button);

    let color_section = GtkBox::new(Orientation::Vertical, 6);
    let color_title = Label::new(Some(&t("Color")));
    color_title.add_css_class("editor-background-section-title");
    color_title.set_xalign(0.0);
    let color = motion_color_control(initial_background_color, "Background color", true, {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        move |rgba| {
            let mut runtime = runtime.borrow_mut();
            runtime.motion.appearance.background_color = [
                rgba.red().into(),
                rgba.green().into(),
                rgba.blue().into(),
                rgba.alpha().into(),
            ];
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    color_section.append(&color_title);
    color_section.append(&color);

    let gradient_section = GtkBox::new(Orientation::Vertical, 6);
    let gradient_title = Label::new(Some(&t("Gradient")));
    gradient_title.add_css_class("editor-background-section-title");
    gradient_title.set_xalign(0.0);
    let gradient = motion_gradient_color_control(initial_gradient_start, initial_gradient_end, {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        move |stop, rgba| {
            let mut runtime = runtime.borrow_mut();
            let color = [
                rgba.red().into(),
                rgba.green().into(),
                rgba.blue().into(),
                rgba.alpha().into(),
            ];
            if stop == 0 {
                runtime.motion.appearance.gradient_color_1 = color;
            } else {
                runtime.motion.appearance.gradient_color_2 = color;
            }
            runtime.motion.appearance.selected_gradient_preset_index = None;
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    gradient_section.append(&gradient_title);
    gradient_section.append(&gradient);
    let (wallpaper_catalog, activate_wallpaper_catalog) =
        motion_wallpaper_catalog_section(session, preview, &none_button);
    let image_section = motion_image_section(
        "Image",
        "Choose Background Image",
        MotionBackgroundFillType::Image,
        window,
        session,
        preview,
        &none_button,
    );

    // Choices remain visible inside one background card. Clicking a choice
    // expands its controls below the list instead of replacing the picker.
    let selection_stack = Stack::new();
    selection_stack.set_hhomogeneous(false);
    selection_stack.set_vhomogeneous(false);
    let color_choice = Button::with_label(&t("Color"));
    let gradient_choice = Button::with_label(&t("Gradient"));
    let wallpapers_choice = Button::with_label(&t("Wallpapers"));
    let image_choice = Button::with_label(&t("Image"));
    for button in [
        &color_choice,
        &gradient_choice,
        &wallpapers_choice,
        &image_choice,
    ] {
        button.set_has_frame(false);
        button.set_halign(Align::Fill);
        button.set_hexpand(true);
        button.add_css_class("editor-background-option-button");
        fill_section.append(button);
    }
    let empty_selection = GtkBox::new(Orientation::Vertical, 0);
    selection_stack.add_named(&empty_selection, Some("empty"));
    selection_stack.add_named(&color_section, Some("color"));
    selection_stack.add_named(&gradient_section, Some("gradient"));
    selection_stack.add_named(&wallpaper_catalog, Some("wallpapers"));
    selection_stack.add_named(&image_section, Some("image"));
    selection_stack.set_visible_child_name("empty");
    fill_section.append(&selection_stack);
    color_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        move |_| {
            runtime.borrow_mut().motion.appearance.background_fill_type =
                MotionBackgroundFillType::Color;
            none_button.remove_css_class("active-background-option");
            color_choice.add_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("color");
            preview.queue_draw();
        }
    });
    gradient_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        move |_| {
            runtime.borrow_mut().motion.appearance.background_fill_type =
                MotionBackgroundFillType::Gradient;
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.add_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("gradient");
            preview.queue_draw();
        }
    });
    wallpapers_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let activate_wallpaper_catalog = activate_wallpaper_catalog.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        move |_| {
            activate_wallpaper_catalog();
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.add_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("wallpapers");
        }
    });
    image_choice.connect_clicked({
        let selection_stack = selection_stack.clone();
        let none_button = none_button.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        move |_| {
            none_button.remove_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.add_css_class("active-background-option");
            selection_stack.set_visible_child_name("image");
        }
    });
    none_button.connect_clicked({
        let selection_stack = selection_stack.clone();
        let color_choice = color_choice.clone();
        let gradient_choice = gradient_choice.clone();
        let wallpapers_choice = wallpapers_choice.clone();
        let image_choice = image_choice.clone();
        let none_button = none_button.clone();
        move |_| {
            none_button.add_css_class("active-background-option");
            color_choice.remove_css_class("active-background-option");
            gradient_choice.remove_css_class("active-background-option");
            wallpapers_choice.remove_css_class("active-background-option");
            image_choice.remove_css_class("active-background-option");
            selection_stack.set_visible_child_name("empty");
        }
    });
    background_section.append(&fill_section);

    let padding = motion_appearance_slider("Padding", 0.0, 200.0, 96.0, "px");
    padding.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.background_padding = slider.value();
            preview.queue_draw();
        }
    });
    background_section.append(&padding.widget());

    let blur = motion_appearance_slider("Background blur", 0.0, 1.0, 0.0, "%");
    blur.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.background_blur = slider.value();
            preview.queue_draw();
        }
    });
    background_section.append(&blur.widget());

    let noise = motion_appearance_slider("Background noise", 0.0, 1.0, 0.0, "%");
    noise.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.background_noise = slider.value();
            preview.queue_draw();
        }
    });
    background_section.append(&noise.widget());
    root.append(&background_section);

    let shadow_section = motion_appearance_section("Shadow");
    let shadow_opacity =
        motion_appearance_slider("Shadow opacity", 0.0, 1.0, initial_shadow_opacity, "%");
    shadow_opacity.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.shadow_opacity = slider.value();
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_opacity.widget());

    let shadow_blur =
        motion_appearance_slider("Shadow blur", 0.0, 120.0, initial_shadow_blur, "px");
    shadow_blur.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.shadow_blur = slider.value();
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_blur.widget());

    let shadow_x = motion_appearance_slider(
        "Shadow position X",
        -200.0,
        200.0,
        initial_shadow_position.0,
        "px",
    );
    shadow_x.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.shadow_position.0 = slider.value();
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_x.widget());

    let shadow_y = motion_appearance_slider(
        "Shadow position Y",
        -200.0,
        200.0,
        initial_shadow_position.1,
        "px",
    );
    shadow_y.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.shadow_position.1 = slider.value();
            preview.queue_draw();
        }
    });
    shadow_section.append(&shadow_y.widget());
    root.append(&shadow_section);

    let border_section = motion_appearance_section("Border");
    let border_title = Label::new(Some(&t("Color")));
    border_title.add_css_class("editor-background-section-title");
    border_title.set_xalign(0.0);
    let border_color = motion_color_control(initial_border, "Border color", false, {
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |rgba| {
            runtime.borrow_mut().motion.appearance.border_fill_color = [
                rgba.red().into(),
                rgba.green().into(),
                rgba.blue().into(),
                rgba.alpha().into(),
            ];
            preview.queue_draw();
        }
    });
    border_section.append(&border_title);
    border_section.append(&border_color);

    let thickness = motion_appearance_slider("Border thickness", 0.0, 24.0, 0.0, "px");
    thickness.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.border_thickness = slider.value();
            preview.queue_draw();
        }
    });
    border_section.append(&thickness.widget());

    // The radius rounds the captured image card itself; the background
    // scene stays a full rectangle.
    let radius = motion_appearance_slider("Border Radius", 0.0, 120.0, 0.0, "px");
    radius.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.appearance.border_radius = slider.value();
            preview.queue_draw();
        }
    });
    border_section.append(&radius.widget());
    root.append(&border_section);
    root
}

fn motion_appearance_section(title: &str) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-motion-settings-section");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("editor-background-section-title");
    heading.set_xalign(0.0);
    section.append(&heading);
    section
}

/// Motion uses the app's bundled background catalog rather than requiring a
/// file chooser for the common Wallpaper path.  The picker intentionally
/// progresses from a short strip → the full grid, keeping the inspector
/// compact until someone asks to browse the catalog.
fn motion_wallpaper_catalog_section(
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
) -> (GtkBox, Rc<dyn Fn()>) {
    let catalog = GtkBox::new(Orientation::Vertical, 6);
    catalog.add_css_class("editor-motion-wallpaper-catalog");
    let stack = Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    catalog.append(&stack);

    let compact = GtkBox::new(Orientation::Vertical, 6);
    let compact_title = Label::new(Some(&t("Wallpapers")));
    compact_title.add_css_class("editor-background-section-title");
    compact_title.set_xalign(0.0);
    compact.append(&compact_title);
    let compact_row = GtkBox::new(Orientation::Horizontal, 8);
    compact_row.add_css_class("editor-motion-wallpaper-row");
    compact.append(&compact_row);
    stack.add_named(&compact, Some("compact"));

    let all = GtkBox::new(Orientation::Vertical, 6);
    let all_header = GtkBox::new(Orientation::Horizontal, 6);
    let all_title = Label::new(Some(&t("Wallpapers")));
    all_title.add_css_class("editor-background-section-title");
    all_title.set_xalign(0.0);
    all_title.set_hexpand(true);
    let show_less = Button::with_label(&t("Show less"));
    show_less.set_has_frame(false);
    show_less.add_css_class("editor-background-section-action-button");
    all_header.append(&all_title);
    all_header.append(&show_less);
    all.append(&all_header);
    let all_grid = GtkBox::new(Orientation::Vertical, 6);
    all_grid.add_css_class("editor-motion-wallpaper-grid");
    all.append(&all_grid);
    stack.add_named(&all, Some("all"));

    let paths: Vec<PathBuf> =
        crate::capture::editor::window::background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES
            .iter()
            .map(|file_name| {
                crate::capture::editor::window::background_panel::background_gradient_asset_path(
                    file_name,
                )
            })
            .filter(|path| path.is_file())
            .collect();
    let selection_buttons = Rc::new(RefCell::new(Vec::<(PathBuf, Button)>::new()));
    for path in paths.iter().take(3) {
        compact_row.append(&motion_wallpaper_thumbnail(
            path,
            session,
            preview,
            none_button,
            selection_buttons.clone(),
        ));
    }
    if let Some(path) = paths.get(3) {
        compact_row.append(&motion_wallpaper_stack_thumbnail(path, &stack));
    }

    for row_paths in paths.chunks(4) {
        let row = GtkBox::new(Orientation::Horizontal, 6);
        row.add_css_class("editor-motion-wallpaper-row");
        for path in row_paths {
            row.append(&motion_wallpaper_thumbnail(
                path,
                session,
                preview,
                none_button,
                selection_buttons.clone(),
            ));
        }
        all_grid.append(&row);
    }

    show_less.connect_clicked({
        let stack = stack.clone();
        move |_| stack.set_visible_child_name("compact")
    });
    stack.set_visible_child_name("compact");
    let activate_catalog: Rc<dyn Fn()> = Rc::new({
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        let default_path = paths.first().cloned();
        move || {
            let Some(default_path) = default_path.clone() else {
                return;
            };
            let selected_path = {
                let mut runtime = session.runtime.borrow_mut();
                if !matches!(
                    runtime.motion.appearance.background_fill_type,
                    MotionBackgroundFillType::Wallpaper
                ) {
                    runtime.motion.appearance.wallpaper_image_name =
                        Some(default_path.to_string_lossy().into_owned());
                    runtime.motion.appearance.background_fill_type =
                        MotionBackgroundFillType::Wallpaper;
                    runtime.background_surface =
                        super::super::motion_render::load_motion_background_surface(
                            &default_path.to_string_lossy(),
                        );
                    none_button.remove_css_class("active-background-option");
                    preview.queue_draw();
                }
                runtime.motion.appearance.wallpaper_image_name.clone()
            };
            for (path, button) in selection_buttons.borrow().iter() {
                if Some(path.to_string_lossy().as_ref()) == selected_path.as_deref() {
                    button.add_css_class("active-background-option");
                } else {
                    button.remove_css_class("active-background-option");
                }
            }
        }
    });
    (catalog, activate_catalog)
}

fn motion_wallpaper_thumbnail(
    path: &std::path::Path,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
    selection_buttons: Rc<RefCell<Vec<(PathBuf, Button)>>>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(48, 48);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-wallpaper-thumbnail");
    button.set_tooltip_text(path.file_stem().and_then(|name| name.to_str()));
    let path = path.to_path_buf();
    let surface =
        super::super::motion_render::load_motion_background_surface(&path.to_string_lossy());
    let thumbnail = motion_wallpaper_thumbnail_area(surface.clone());
    button.set_child(Some(&thumbnail));

    if session
        .runtime
        .borrow()
        .motion
        .appearance
        .wallpaper_image_name
        .as_deref()
        == Some(path.to_string_lossy().as_ref())
    {
        button.add_css_class("active-background-option");
    }
    selection_buttons
        .borrow_mut()
        .push((path.clone(), button.clone()));
    button.connect_clicked({
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let selection_buttons = selection_buttons.clone();
        move |_| {
            let mut runtime = session.runtime.borrow_mut();
            runtime.motion.appearance.wallpaper_image_name =
                Some(path.to_string_lossy().into_owned());
            runtime.motion.appearance.background_fill_type = MotionBackgroundFillType::Wallpaper;
            runtime.background_surface = surface.clone();
            for (candidate_path, candidate) in selection_buttons.borrow().iter() {
                if candidate_path == &path {
                    candidate.add_css_class("active-background-option");
                } else {
                    candidate.remove_css_class("active-background-option");
                }
            }
            none_button.remove_css_class("active-background-option");
            preview.queue_draw();
        }
    });
    button
}

/// The fourth compact preview is the catalog's visual stack affordance.  It
/// remains a wallpaper thumbnail, with a small overlay indicating that it
/// opens the complete collection rather than selecting that particular image.
fn motion_wallpaper_stack_thumbnail(path: &std::path::Path, stack: &Stack) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_size_request(48, 48);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-preview-size-regular");
    button.add_css_class("editor-motion-wallpaper-thumbnail");
    button.add_css_class("editor-motion-wallpaper-stack-thumbnail");
    button.set_tooltip_text(Some(&t("Show all wallpapers")));
    let surface =
        super::super::motion_render::load_motion_background_surface(&path.to_string_lossy());
    let thumbnail = motion_wallpaper_thumbnail_area(surface);
    let overlay = Overlay::new();
    overlay.set_child(Some(&thumbnail));
    let expand = Label::new(Some("⌄"));
    expand.set_halign(Align::Center);
    expand.set_valign(Align::Center);
    expand.set_can_target(false);
    expand.add_css_class("editor-motion-wallpaper-stack-glyph");
    overlay.add_overlay(&expand);
    button.set_child(Some(&overlay));
    button.connect_clicked({
        let stack = stack.clone();
        move |_| stack.set_visible_child_name("all")
    });
    button
}

fn motion_wallpaper_thumbnail_area(surface: Option<gtk4::cairo::ImageSurface>) -> DrawingArea {
    let thumbnail = DrawingArea::new();
    thumbnail.set_content_width(48);
    thumbnail.set_content_height(48);
    thumbnail.set_draw_func(move |_, context, width, height| {
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let source_w = f64::from(surface.width().max(1));
        let source_h = f64::from(surface.height().max(1));
        let scale = (f64::from(width) / source_w).max(f64::from(height) / source_h);
        let _ = context.save();
        motion_thumbnail_rounded_rectangle(
            context,
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
            11.0,
        );
        context.clip();
        context.translate(
            (f64::from(width) - source_w * scale) * 0.5,
            (f64::from(height) - source_h * scale) * 0.5,
        );
        context.scale(scale, scale);
        context.set_source_surface(surface, 0.0, 0.0).ok();
        context.paint().ok();
        context.restore().ok();
    });
    thumbnail
}

fn motion_thumbnail_rounded_rectangle(
    context: &Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let radius = radius.min(width.min(height) * 0.5).max(0.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2 * 3.0,
    );
    context.close_path();
}

fn motion_image_section(
    title: &str,
    dialog_title: &str,
    kind: MotionBackgroundFillType,
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
    none_button: &Button,
) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 6);
    let label = Label::new(Some(&t(title)));
    label.add_css_class("editor-background-section-title");
    label.set_xalign(0.0);
    let choose = Button::with_label(&t("Choose…"));
    choose.set_has_frame(false);
    choose.add_css_class("editor-sidebar-action-button");
    choose.connect_clicked({
        let window = window.downgrade();
        let session = session.clone();
        let preview = preview.clone();
        let none_button = none_button.clone();
        let dialog_title = dialog_title.to_string();
        move |_| {
            let chooser = FileChooserNative::new(
                Some(&dialog_title),
                window.upgrade().as_ref(),
                FileChooserAction::Open,
                Some(&t("Choose")),
                Some(&t("Cancel")),
            );
            let filter = FileFilter::new();
            filter.set_name(Some(&t("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let session = session.clone();
            let preview = preview.clone();
            let kind = kind.clone();
            let none_button = none_button.clone();
            chooser.connect_response(move |dialog, response| {
                if response != ResponseType::Accept {
                    return;
                }
                let Some(path) = dialog.file().and_then(|file| file.path()) else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                let mut runtime = session.runtime.borrow_mut();
                match kind {
                    MotionBackgroundFillType::Wallpaper => {
                        runtime.motion.appearance.wallpaper_image_name = Some(path);
                    }
                    MotionBackgroundFillType::Image => {
                        runtime.motion.appearance.custom_background_image = Some(path);
                    }
                    _ => return,
                }
                runtime.motion.appearance.background_fill_type = kind.clone();
                let active_path = match kind {
                    MotionBackgroundFillType::Wallpaper => {
                        runtime.motion.appearance.wallpaper_image_name.as_deref()
                    }
                    MotionBackgroundFillType::Image => {
                        runtime.motion.appearance.custom_background_image.as_deref()
                    }
                    _ => None,
                };
                runtime.background_surface = active_path
                    .and_then(super::super::motion_render::load_motion_background_surface);
                none_button.remove_css_class("active-background-option");
                preview.queue_draw();
            });
            chooser.show();
        }
    });
    section.append(&label);
    section.append(&choose);
    section
}
