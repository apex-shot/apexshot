use gtk4::{
    glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, CheckButton, DrawingArea,
    FileChooserAction, FileChooserNative, FileFilter, Label, Orientation, ResponseType, Scale,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use super::super::render::rgba_image_to_surface;

pub(super) const BACKGROUND_SIDEBAR_WIDTH: i32 = 228;
pub(super) const BACKGROUND_GRADIENT_PREVIEW_SIZE: u32 = 96;
pub(super) const BACKGROUND_GRADIENT_PREVIEW_FILES: [&str; 10] = [
    "codioful-formerly-gradienta-n2XqPm7Bqhk-unsplash.jpg",
    "codioful-formerly-gradienta-O10vBIDRkZw-unsplash.jpg",
    "kunal-patil-2hB-jhXLd3c-unsplash.jpg",
    "kunal-patil-8ZKlgI_G-mw-unsplash.jpg",
    "luke-chesser-CxBx_J3yp9g-unsplash.jpg",
    "luke-chesser-pJadQetzTkI-unsplash.jpg",
    "magicpattern-87PP9Zd7MNo-unsplash.jpg",
    "magicpattern-bevXKKL7E9g-unsplash.jpg",
    "magicpattern-oPH_5xuMgQw-unsplash.jpg",
    "milad-fakurian-nY14Fs8pxT8-unsplash.jpg",
];
const BACKGROUND_GRADIENT_PREVIEW_CLASSES: [&str; 10] = [
    "editor-background-gradient-preview-1",
    "editor-background-gradient-preview-2",
    "editor-background-gradient-preview-3",
    "editor-background-gradient-preview-4",
    "editor-background-gradient-preview-5",
    "editor-background-gradient-preview-6",
    "editor-background-gradient-preview-7",
    "editor-background-gradient-preview-8",
    "editor-background-gradient-preview-9",
    "editor-background-gradient-preview-10",
];
const BACKGROUND_PLAIN_COLOR_CLASSES: [&str; 18] = [
    "editor-background-plain-color-1",
    "editor-background-plain-color-2",
    "editor-background-plain-color-3",
    "editor-background-plain-color-4",
    "editor-background-plain-color-5",
    "editor-background-plain-color-6",
    "editor-background-plain-color-7",
    "editor-background-plain-color-8",
    "editor-background-plain-color-9",
    "editor-background-plain-color-10",
    "editor-background-plain-color-11",
    "editor-background-plain-color-12",
    "editor-background-plain-color-13",
    "editor-background-plain-color-14",
    "editor-background-plain-color-15",
    "editor-background-plain-color-16",
    "editor-background-plain-color-17",
    "editor-background-plain-color-18",
];

pub(super) fn background_gradient_asset_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/capture/editor/background-images")
        .join(file_name)
}

pub(super) fn load_background_preview_image(path: &Path, preview_size: u32) -> Option<RgbaImage> {
    let image = image::open(path).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let square_size = width.min(height);
    let crop_x = (width - square_size) / 2;
    let crop_y = (height - square_size) / 2;
    let cropped =
        image::imageops::crop_imm(&image, crop_x, crop_y, square_size, square_size).to_image();

    Some(image::imageops::resize(
        &cropped,
        preview_size,
        preview_size,
        image::imageops::FilterType::Triangle,
    ))
}

pub(super) fn load_background_gradient_preview_image(
    file_name: &str,
    preview_size: u32,
) -> Option<RgbaImage> {
    load_background_preview_image(&background_gradient_asset_path(file_name), preview_size)
}

fn parse_wallpaper_setting(raw_value: &str) -> Option<PathBuf> {
    let trimmed = raw_value.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }

    if let Ok(uri) = url::Url::parse(trimmed) {
        if uri.scheme() == "file" {
            return uri.to_file_path().ok().filter(|path| path.is_file());
        }
    }

    let path = PathBuf::from(trimmed);
    path.is_file().then_some(path)
}

pub(super) fn detect_system_wallpaper_path() -> Option<PathBuf> {
    let setting_queries = [
        ("org.gnome.desktop.background", "picture-uri-dark"),
        ("org.gnome.desktop.background", "picture-uri"),
        ("org.cinnamon.desktop.background", "picture-uri"),
        ("org.mate.background", "picture-filename"),
    ];

    for (schema, key) in setting_queries {
        let output = match Command::new("gsettings")
            .arg("get")
            .arg(schema)
            .arg(key)
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = parse_wallpaper_setting(&stdout) {
            return Some(path);
        }
    }

    None
}

fn draw_preview_tile_surface(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
    corner_radius: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let radius = corner_radius.min(width * 0.5).min(height * 0.5);

    context.new_path();
    context.arc(
        width - radius,
        radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        width - radius,
        height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        radius,
        height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        radius,
        radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
    context.clip();

    let surface_width = surface.width().max(1) as f64;
    let surface_height = surface.height().max(1) as f64;
    let scale = (width / surface_width).max(height / surface_height);
    let draw_width = surface_width * scale;
    let draw_height = surface_height * scale;
    let offset_x = (width - draw_width) * 0.5;
    let offset_y = (height - draw_height) * 0.5;

    let _ = context.save();
    context.translate(offset_x, offset_y);
    context.scale(scale, scale);
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

pub(super) fn build_background_gradient_preview_button(
    index: usize,
) -> (
    Button,
    DrawingArea,
    Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>,
) {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class(BACKGROUND_GRADIENT_PREVIEW_CLASSES[index]);
    button.set_tooltip_text(Some("Gradient"));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    let preview_surface = Rc::new(RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let preview_surface_draw = preview_surface.clone();
    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = preview_surface_draw.borrow().as_ref() {
            draw_preview_tile_surface(context, surface, width, height, 12.0);
        }
    });

    button.set_child(Some(&preview_area));

    (button, preview_area, preview_surface)
}

fn build_background_wallpaper_preview_button(
    surface: Option<gtk4::cairo::ImageSurface>,
    tooltip: &str,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.set_tooltip_text(Some(tooltip));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = surface.as_ref() {
            draw_preview_tile_surface(context, surface, width, height, 12.0);
        }
    });

    button.set_child(Some(&preview_area));
    button
}

pub(super) fn build_background_add_wallpaper_button() -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-add-button");
    button.set_tooltip_text(Some("Add wallpaper"));

    let plus_label = Label::new(Some("+"));
    plus_label.add_css_class("editor-background-add-label");
    button.set_child(Some(&plus_label));

    button
}

pub(super) fn build_background_blurred_preview_button(index: usize) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-blurred-button");
    button.set_tooltip_text(Some(&format!("Blurred {}", index + 1)));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    button.set_child(Some(&preview_area));
    button
}

fn build_background_plain_color_button(index: usize) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.set_size_request(18, 18);
    button.add_css_class("editor-background-plain-color-button");
    button.add_css_class(BACKGROUND_PLAIN_COLOR_CLASSES[index]);
    button.set_tooltip_text(Some(&format!("Plain color {}", index + 1)));
    button
}

pub(super) fn build_background_plain_color_cell(index: usize) -> GtkBox {
    let cell = GtkBox::new(Orientation::Vertical, 0);
    cell.add_css_class("editor-background-plain-color-cell");
    cell.set_hexpand(true);
    cell.set_halign(gtk4::Align::Fill);
    cell.set_valign(gtk4::Align::Center);
    cell.append(&build_background_plain_color_button(index));
    cell
}

pub(super) fn rebuild_wallpaper_preview_grid(
    wallpaper_grid: &GtkBox,
    wallpaper_previews: &[(String, Option<gtk4::cairo::ImageSurface>)],
    add_button: &Button,
) {
    while let Some(child) = wallpaper_grid.first_child() {
        wallpaper_grid.remove(&child);
    }

    let new_row = || {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.add_css_class("editor-background-wallpaper-row");
        row
    };

    let mut row = new_row();
    let mut items_in_row = 0usize;

    for (tooltip, surface) in wallpaper_previews {
        if items_in_row == 5 {
            wallpaper_grid.append(&row);
            row = new_row();
            items_in_row = 0;
        }

        let preview_button = build_background_wallpaper_preview_button(surface.clone(), tooltip);
        row.append(&preview_button);
        items_in_row += 1;
    }

    if items_in_row == 5 {
        wallpaper_grid.append(&row);
        row = new_row();
    }

    row.append(add_button);
    wallpaper_grid.append(&row);
}

pub(super) struct BackgroundPanelParts {
    pub sidebar: GtkBox,
    pub start_gradient_preview_loading: Rc<dyn Fn()>,
}

pub(super) fn build_background_panel(window: &ApplicationWindow) -> BackgroundPanelParts {
    let background_sidebar = GtkBox::new(Orientation::Vertical, 10);
    background_sidebar.add_css_class("editor-background-sidebar");
    background_sidebar.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    background_sidebar.set_hexpand(false);
    background_sidebar.set_visible(false);
    background_sidebar.set_vexpand(true);

    let background_sidebar_options = GtkBox::new(Orientation::Vertical, 8);
    background_sidebar_options.add_css_class("editor-background-sidebar-options");

    let background_none_btn = Button::with_label("None");
    background_none_btn.set_has_frame(false);
    background_none_btn.set_halign(gtk4::Align::Start);
    background_none_btn.set_hexpand(false);
    background_none_btn.set_size_request(312, -1);
    background_none_btn.add_css_class("editor-background-option-button");
    background_none_btn.add_css_class("active-background-option");

    let gradients_section = GtkBox::new(Orientation::Vertical, 10);
    gradients_section.add_css_class("editor-background-gradients-section");

    let gradients_title = Label::new(Some("Gradients"));
    gradients_title.add_css_class("editor-background-section-title");
    gradients_title.set_xalign(0.0);

    let gradients_grid = GtkBox::new(Orientation::Vertical, 8);
    gradients_grid.add_css_class("editor-background-gradients-grid");

    let mut background_preview_areas: Vec<DrawingArea> =
        Vec::with_capacity(BACKGROUND_GRADIENT_PREVIEW_FILES.len());
    let mut background_preview_surfaces: Vec<Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>> =
        Vec::with_capacity(BACKGROUND_GRADIENT_PREVIEW_FILES.len());

    for (row_index, chunk) in BACKGROUND_GRADIENT_PREVIEW_FILES.chunks(5).enumerate() {
        let gradient_row = GtkBox::new(Orientation::Horizontal, 8);
        gradient_row.add_css_class("editor-background-gradients-row");

        for (column_index, _) in chunk.iter().enumerate() {
            let preview_index = row_index * 5 + column_index;
            let (preview_button, preview_area, preview_surface) =
                build_background_gradient_preview_button(preview_index);
            background_preview_areas.push(preview_area);
            background_preview_surfaces.push(preview_surface);
            gradient_row.append(&preview_button);
        }

        gradients_grid.append(&gradient_row);
    }

    let background_preview_areas = Rc::new(background_preview_areas);
    let background_preview_surfaces = Rc::new(background_preview_surfaces);
    let background_gradient_previews_started = Rc::new(Cell::new(false));
    let start_background_gradient_preview_loading: Rc<dyn Fn()> = Rc::new({
        let background_preview_areas = background_preview_areas.clone();
        let background_preview_surfaces = background_preview_surfaces.clone();
        let background_gradient_previews_started = background_gradient_previews_started.clone();
        move || {
            if background_gradient_previews_started.replace(true) {
                return;
            }

            let (sender, receiver) = std::sync::mpsc::channel::<(usize, RgbaImage)>();
            std::thread::spawn(move || {
                for (index, file_name) in BACKGROUND_GRADIENT_PREVIEW_FILES.iter().enumerate() {
                    if let Some(preview_image) = load_background_gradient_preview_image(
                        file_name,
                        BACKGROUND_GRADIENT_PREVIEW_SIZE,
                    ) {
                        if sender.send((index, preview_image)).is_err() {
                            break;
                        }
                    }
                }
            });

            let background_preview_areas = background_preview_areas.clone();
            let background_preview_surfaces = background_preview_surfaces.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || loop {
                match receiver.try_recv() {
                    Ok((index, preview_image)) => {
                        if let Some(surface) = rgba_image_to_surface(&preview_image) {
                            *background_preview_surfaces[index].borrow_mut() = Some(surface);
                            background_preview_areas[index].queue_draw();
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return glib::ControlFlow::Break;
                    }
                }
            });
        }
    });

    gradients_section.append(&gradients_title);
    gradients_section.append(&gradients_grid);

    let wallpaper_section = GtkBox::new(Orientation::Vertical, 10);
    wallpaper_section.add_css_class("editor-background-wallpaper-section");

    let wallpaper_title = Label::new(Some("Wallpaper"));
    wallpaper_title.add_css_class("editor-background-section-title");
    wallpaper_title.set_xalign(0.0);

    let wallpaper_grid = GtkBox::new(Orientation::Vertical, 8);
    wallpaper_grid.add_css_class("editor-background-wallpaper-grid");

    let wallpaper_preview_path = detect_system_wallpaper_path()
        .unwrap_or_else(|| background_gradient_asset_path(BACKGROUND_GRADIENT_PREVIEW_FILES[0]));
    let wallpaper_preview_surface =
        load_background_preview_image(&wallpaper_preview_path, BACKGROUND_GRADIENT_PREVIEW_SIZE)
            .and_then(|preview_image| rgba_image_to_surface(&preview_image));
    let wallpaper_preview_label = wallpaper_preview_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(|file_name| format!("Wallpaper: {file_name}"))
        .unwrap_or_else(|| "Wallpaper".to_string());
    let wallpaper_previews = Rc::new(RefCell::new(vec![(
        wallpaper_preview_label,
        wallpaper_preview_surface,
    )]));
    let add_wallpaper_btn = build_background_add_wallpaper_button();

    {
        let previews = wallpaper_previews.borrow();
        rebuild_wallpaper_preview_grid(&wallpaper_grid, previews.as_slice(), &add_wallpaper_btn);
    }

    add_wallpaper_btn.connect_clicked({
        let wallpaper_previews = wallpaper_previews.clone();
        let wallpaper_grid = wallpaper_grid.clone();
        let add_wallpaper_btn = add_wallpaper_btn.clone();
        let window_weak = window.downgrade();
        move |_| {
            let chooser = FileChooserNative::new(
                Some("Choose wallpaper image"),
                window_weak.upgrade().as_ref(),
                FileChooserAction::Open,
                Some("Add"),
                Some("Cancel"),
            );

            let filter = FileFilter::new();
            filter.set_name(Some("Images"));
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            filter.add_mime_type("image/webp");
            filter.add_mime_type("image/gif");
            filter.add_pattern("*.png");
            filter.add_pattern("*.jpg");
            filter.add_pattern("*.jpeg");
            filter.add_pattern("*.webp");
            filter.add_pattern("*.gif");
            chooser.add_filter(&filter);

            chooser.connect_response({
                let wallpaper_previews = wallpaper_previews.clone();
                let wallpaper_grid = wallpaper_grid.clone();
                let add_wallpaper_btn = add_wallpaper_btn.clone();
                move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                if let Some(surface) = load_background_preview_image(
                                    &path,
                                    BACKGROUND_GRADIENT_PREVIEW_SIZE,
                                )
                                .and_then(|preview_image| rgba_image_to_surface(&preview_image))
                                {
                                    let label = path
                                        .file_name()
                                        .and_then(|file_name| file_name.to_str())
                                        .map(|file_name| format!("Wallpaper: {file_name}"))
                                        .unwrap_or_else(|| "Wallpaper".to_string());
                                    wallpaper_previews.borrow_mut().push((label, Some(surface)));
                                    let previews = wallpaper_previews.borrow();
                                    rebuild_wallpaper_preview_grid(
                                        &wallpaper_grid,
                                        previews.as_slice(),
                                        &add_wallpaper_btn,
                                    );
                                }
                            }
                        }
                    }

                    dialog.hide();
                }
            });

            chooser.show();
        }
    });

    wallpaper_section.append(&wallpaper_title);
    wallpaper_section.append(&wallpaper_grid);

    let blurred_section = GtkBox::new(Orientation::Vertical, 10);
    blurred_section.add_css_class("editor-background-blurred-section");

    let blurred_title = Label::new(Some("Blurred"));
    blurred_title.add_css_class("editor-background-section-title");
    blurred_title.set_xalign(0.0);

    let blurred_row = GtkBox::new(Orientation::Horizontal, 8);
    blurred_row.add_css_class("editor-background-blurred-row");

    for index in 0..3 {
        let blurred_button = build_background_blurred_preview_button(index);
        blurred_row.append(&blurred_button);
    }

    blurred_section.append(&blurred_title);
    blurred_section.append(&blurred_row);

    let plain_color_section = GtkBox::new(Orientation::Vertical, 10);
    plain_color_section.add_css_class("editor-background-plain-color-section");

    let plain_color_title = Label::new(Some("Plain color"));
    plain_color_title.add_css_class("editor-background-section-title");
    plain_color_title.set_xalign(0.0);

    let plain_color_grid = GtkBox::new(Orientation::Vertical, 8);
    plain_color_grid.add_css_class("editor-background-plain-color-grid");

    for row_index in 0..2 {
        let plain_color_row = GtkBox::new(Orientation::Horizontal, 8);
        plain_color_row.add_css_class("editor-background-plain-color-row");
        plain_color_row.set_hexpand(true);
        plain_color_row.set_homogeneous(true);

        for column_index in 0..9 {
            let color_index = row_index * 9 + column_index;
            let color_cell = build_background_plain_color_cell(color_index);
            plain_color_row.append(&color_cell);
        }

        let row_end_spacer = GtkBox::new(Orientation::Horizontal, 0);
        row_end_spacer.add_css_class("editor-background-plain-color-end-spacer");
        plain_color_row.append(&row_end_spacer);

        plain_color_grid.append(&plain_color_row);
    }

    plain_color_section.append(&plain_color_title);
    plain_color_section.append(&plain_color_grid);

    let background_padding_divider_row = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider_row.add_css_class("editor-background-divider-row");
    let background_padding_divider = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider.add_css_class("editor-background-divider");
    background_padding_divider.set_size_request(252, -1);
    background_padding_divider_row.append(&background_padding_divider);

    let padding_section = GtkBox::new(Orientation::Vertical, 10);
    padding_section.add_css_class("editor-background-padding-section");

    let padding_title = Label::new(Some("Padding"));
    padding_title.add_css_class("editor-background-section-title");
    padding_title.set_xalign(0.0);

    let padding_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    padding_slider_row.add_css_class("editor-background-padding-slider-row");
    let padding_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    padding_slider.add_css_class("editor-opacity-slider");
    padding_slider.add_css_class("editor-background-padding-slider");
    padding_slider.set_draw_value(false);
    padding_slider.set_value(24.0);
    padding_slider.set_size_request(252, -1);
    padding_slider_row.append(&padding_slider);

    padding_section.append(&padding_title);
    padding_section.append(&padding_slider_row);

    let compact_controls = GtkBox::new(Orientation::Vertical, 8);
    compact_controls.add_css_class("editor-background-compact-controls");

    let insert_shadow_row = GtkBox::new(Orientation::Horizontal, 16);
    insert_shadow_row.add_css_class("editor-background-compact-controls-row");
    insert_shadow_row.set_homogeneous(true);

    let insert_section = GtkBox::new(Orientation::Vertical, 10);
    insert_section.add_css_class("editor-background-compact-slider-section");

    let insert_title = Label::new(Some("Insert"));
    insert_title.add_css_class("editor-background-section-title");
    insert_title.set_xalign(0.0);

    let insert_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    insert_slider_row.add_css_class("editor-background-compact-slider-row");
    let insert_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    insert_slider.add_css_class("editor-opacity-slider");
    insert_slider.add_css_class("editor-background-compact-slider");
    insert_slider.set_draw_value(false);
    insert_slider.set_value(20.0);
    insert_slider.set_size_request(136, -1);
    insert_slider_row.append(&insert_slider);

    insert_section.append(&insert_title);
    insert_section.append(&insert_slider_row);

    let auto_balance_section = GtkBox::new(Orientation::Vertical, 2);
    auto_balance_section.add_css_class("editor-background-compact-slider-section");

    let auto_balance_title = Label::new(Some("Auto-balance"));
    auto_balance_title.add_css_class("editor-background-section-title");
    auto_balance_title.set_xalign(0.0);

    let auto_balance_check_row = GtkBox::new(Orientation::Horizontal, 0);
    auto_balance_check_row.add_css_class("editor-background-checkbox-row");
    let auto_balance_check = CheckButton::with_label("");
    auto_balance_check.add_css_class("editor-background-checkbox");
    auto_balance_check.set_halign(gtk4::Align::Start);
    auto_balance_check_row.append(&auto_balance_check);

    auto_balance_section.append(&auto_balance_title);
    auto_balance_section.append(&auto_balance_check_row);

    insert_shadow_row.append(&insert_section);
    insert_shadow_row.append(&auto_balance_section);

    let shadow_corners_row = GtkBox::new(Orientation::Horizontal, 16);
    shadow_corners_row.add_css_class("editor-background-compact-controls-row");
    shadow_corners_row.set_homogeneous(true);

    let shadow_section = GtkBox::new(Orientation::Vertical, 10);
    shadow_section.add_css_class("editor-background-compact-slider-section");

    let shadow_title = Label::new(Some("Shadow"));
    shadow_title.add_css_class("editor-background-section-title");
    shadow_title.set_xalign(0.0);

    let shadow_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    shadow_slider_row.add_css_class("editor-background-compact-slider-row");
    let shadow_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    shadow_slider.add_css_class("editor-opacity-slider");
    shadow_slider.add_css_class("editor-background-compact-slider");
    shadow_slider.set_draw_value(false);
    shadow_slider.set_value(28.0);
    shadow_slider.set_size_request(136, -1);
    shadow_slider_row.append(&shadow_slider);

    shadow_section.append(&shadow_title);
    shadow_section.append(&shadow_slider_row);

    let alignment_section = GtkBox::new(Orientation::Vertical, 10);
    alignment_section.add_css_class("editor-background-compact-slider-section");

    let alignment_title = Label::new(Some("Alignment"));
    alignment_title.add_css_class("editor-background-section-title");
    alignment_title.set_xalign(0.0);

    let alignment_grid = GtkBox::new(Orientation::Vertical, 6);
    alignment_grid.add_css_class("editor-background-alignment-grid");
    alignment_grid.set_halign(gtk4::Align::Start);

    let alignment_positions = [
        [
            ("top-left", "Top left"),
            ("top-center", "Top middle"),
            ("top-right", "Top right"),
        ],
        [
            ("center-left", "Left"),
            ("center", "Middle"),
            ("center-right", "Right"),
        ],
        [
            ("bottom-left", "Bottom left"),
            ("bottom-center", "Bottom middle"),
            ("bottom-right", "Bottom right"),
        ],
    ];

    for row_items in alignment_positions {
        let alignment_row = GtkBox::new(Orientation::Horizontal, 6);
        alignment_row.add_css_class("editor-background-alignment-row");
        alignment_row.set_homogeneous(true);

        for (position_class, tooltip) in row_items {
            let alignment_frame = GtkBox::new(Orientation::Horizontal, 0);
            alignment_frame.add_css_class("editor-background-alignment-icon-frame");
            alignment_frame.add_css_class(position_class);
            alignment_frame.set_size_request(6, 4);

            match position_class {
                "top-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "top-center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "top-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "center-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "center-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "bottom-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                "bottom-center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                "bottom-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                _ => {}
            }

            let alignment_icon = gtk4::Overlay::new();
            alignment_icon.add_css_class("editor-background-alignment-icon");
            alignment_icon.set_size_request(26, 17);
            alignment_icon.set_child(Some(&alignment_frame));

            let alignment_button = Button::new();
            alignment_button.set_child(Some(&alignment_icon));
            alignment_button.set_has_frame(false);
            alignment_button.set_focusable(false);
            alignment_button.set_hexpand(true);
            alignment_button.set_tooltip_text(Some(tooltip));
            alignment_button.add_css_class("editor-background-alignment-button");
            alignment_row.append(&alignment_button);
        }

        alignment_grid.append(&alignment_row);
    }

    alignment_section.append(&alignment_title);
    alignment_section.append(&alignment_grid);
    shadow_section.append(&alignment_section);

    let corners_section = GtkBox::new(Orientation::Vertical, 10);
    corners_section.add_css_class("editor-background-compact-slider-section");

    let ratio_section = GtkBox::new(Orientation::Vertical, 10);
    ratio_section.add_css_class("editor-background-compact-slider-section");

    let ratio_title = Label::new(Some("Ratio"));
    ratio_title.add_css_class("editor-background-section-title");
    ratio_title.set_xalign(0.0);

    let ratio_dropdown_row = GtkBox::new(Orientation::Horizontal, 0);
    ratio_dropdown_row.add_css_class("editor-background-ratio-dropdown-row");
    let ratio_dropdown = gtk4::DropDown::from_strings(&["Original", "1:1", "4:3", "16:9", "21:9"]);
    ratio_dropdown.add_css_class("editor-background-ratio-dropdown");
    ratio_dropdown.set_size_request(130, -1);
    ratio_dropdown.set_halign(gtk4::Align::Start);
    ratio_dropdown.set_selected(0);
    ratio_dropdown_row.append(&ratio_dropdown);

    ratio_section.append(&ratio_title);
    ratio_section.append(&ratio_dropdown_row);

    let corners_title = Label::new(Some("Corners"));
    corners_title.add_css_class("editor-background-section-title");
    corners_title.set_xalign(0.0);

    let corners_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    corners_slider_row.add_css_class("editor-background-compact-slider-row");
    let corners_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    corners_slider.add_css_class("editor-opacity-slider");
    corners_slider.add_css_class("editor-background-compact-slider");
    corners_slider.set_draw_value(false);
    corners_slider.set_value(18.0);
    corners_slider.set_size_request(136, -1);
    corners_slider_row.append(&corners_slider);

    corners_section.append(&corners_title);
    corners_section.append(&corners_slider_row);
    corners_section.append(&ratio_section);

    shadow_corners_row.append(&shadow_section);
    shadow_corners_row.append(&corners_section);

    compact_controls.append(&insert_shadow_row);
    compact_controls.append(&shadow_corners_row);

    background_sidebar_options.append(&background_none_btn);
    background_sidebar_options.append(&gradients_section);
    background_sidebar_options.append(&wallpaper_section);
    background_sidebar_options.append(&blurred_section);
    background_sidebar_options.append(&plain_color_section);
    background_sidebar_options.append(&background_padding_divider_row);
    background_sidebar_options.append(&padding_section);
    background_sidebar_options.append(&compact_controls);

    background_sidebar.append(&background_sidebar_options);

    BackgroundPanelParts {
        sidebar: background_sidebar,
        start_gradient_preview_loading: start_background_gradient_preview_loading,
    }
}
