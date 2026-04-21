use gtk4::{
    glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, CheckButton, DrawingArea,
    FileChooserAction, FileChooserNative, FileFilter, Label, Orientation, ResponseType, Scale,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::render::rgba_image_to_surface;
use super::super::state::EditorState;
use super::super::types::{BackgroundAlignment, BackgroundStyle, CropAspectRatio};

pub(super) const BACKGROUND_SIDEBAR_WIDTH: i32 = 210;
pub(super) const BACKGROUND_GRADIENT_PREVIEW_SIZE: u32 = 96;
pub(super) const MAX_BACKGROUND_DIMENSION: u32 = 2560;
const BACKGROUND_PREVIEW_COLUMNS: usize = 5;

const fn preview_grid_width(preview_button_size: i32, preview_row_spacing: i32) -> i32 {
    preview_button_size * BACKGROUND_PREVIEW_COLUMNS as i32
        + preview_row_spacing * (BACKGROUND_PREVIEW_COLUMNS as i32 - 1)
}

#[derive(Clone, Copy)]
pub(super) struct BackgroundSidebarDensity {
    preview_button_size: i32,
    preview_draw_size: i32,
    preview_corner_radius: i32,
    preview_row_spacing: i32,
    section_spacing: i32,
    content_spacing: i32,
    divider_width: i32,
    wide_slider_width: i32,
    compact_slider_width: i32,
    none_button_width: i32,
    preview_size_class: &'static str,
}

impl BackgroundSidebarDensity {
    fn for_gradient_count(gradient_count: usize) -> Self {
        if gradient_count >= 15 {
            let width = preview_grid_width(36, 4);
            Self {
                preview_button_size: 36,
                preview_draw_size: 36,
                preview_corner_radius: 8,
                preview_row_spacing: 4,
                section_spacing: 6,
                content_spacing: 4,
                divider_width: width,
                wide_slider_width: width,
                compact_slider_width: (width - 24) / 2,
                none_button_width: width,
                preview_size_class: "editor-background-preview-size-compact",
            }
        } else if gradient_count >= 10 {
            let width = preview_grid_width(42, 5);
            Self {
                preview_button_size: 42,
                preview_draw_size: 42,
                preview_corner_radius: 10,
                preview_row_spacing: 5,
                section_spacing: 7,
                content_spacing: 6,
                divider_width: width,
                wide_slider_width: width,
                compact_slider_width: (width - 24) / 2,
                none_button_width: width,
                preview_size_class: "editor-background-preview-size-medium",
            }
        } else {
            let width = preview_grid_width(48, 6);
            Self {
                preview_button_size: 48,
                preview_draw_size: 48,
                preview_corner_radius: 11,
                preview_row_spacing: 6,
                section_spacing: 8,
                content_spacing: 7,
                divider_width: width,
                wide_slider_width: width,
                compact_slider_width: (width - 24) / 2,
                none_button_width: width,
                preview_size_class: "editor-background-preview-size-regular",
            }
        }
    }
}

pub const BACKGROUND_GRADIENT_PREVIEW_FILES: [&str; 20] = [
    "gradient-01.jpg",
    "gradient-02.jpg",
    "gradient-03.jpg",
    "gradient-04.jpg",
    "gradient-05.jpg",
    "gradient-06.jpg",
    "gradient-07.jpg",
    "gradient-08.jpg",
    "gradient-09.jpg",
    "gradient-10.jpg",
    "gradient-11.jpg",
    "gradient-12.jpg",
    "gradient-13.jpg",
    "gradient-14.jpg",
    "gradient-15.jpg",
    "gradient-16.jpg",
    "gradient-17.jpg",
    "gradient-18.jpg",
    "gradient-19.jpg",
    "gradient-20.jpg",
];
const BACKGROUND_GRADIENT_PREVIEW_CLASSES: [&str; 20] = [
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
    "editor-background-gradient-preview-11",
    "editor-background-gradient-preview-12",
    "editor-background-gradient-preview-13",
    "editor-background-gradient-preview-14",
    "editor-background-gradient-preview-15",
    "editor-background-gradient-preview-16",
    "editor-background-gradient-preview-17",
    "editor-background-gradient-preview-18",
    "editor-background-gradient-preview-19",
    "editor-background-gradient-preview-20",
];
pub fn background_gradient_asset_path(file_name: &str) -> PathBuf {
    let asset_paths = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/capture/editor/background-images")
            .join(file_name),
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("background-images").join(file_name))
            })
            .unwrap_or_default(),
        PathBuf::from("/usr/share/apexshot/background-images").join(file_name),
        PathBuf::from("/usr/local/share/apexshot/background-images").join(file_name),
    ];

    asset_paths
        .into_iter()
        .find(|path| !path.as_os_str().is_empty() && path.exists())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src/capture/editor/background-images")
                .join(file_name)
        })
}
pub(super) fn load_background_preview_image(path: &Path, preview_size: u32) -> Option<RgbaImage> {
    let img = match image::io::Reader::open(path) {
        Ok(reader) => match reader.with_guessed_format() {
            Ok(reader) => reader.decode().ok(),
            Err(_) => None,
        },
        Err(_) => None,
    }?;

    let image = img.into_rgba8();
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

pub fn load_background_image_optimized(path: &Path) -> Option<RgbaImage> {
    let img = match image::io::Reader::open(path) {
        Ok(reader) => match reader.with_guessed_format() {
            Ok(reader) => reader.decode().ok(),
            Err(_) => None,
        },
        Err(_) => None,
    }?;

    let image = img.into_rgba8();
    let (width, height) = image.dimensions();

    if width > MAX_BACKGROUND_DIMENSION || height > MAX_BACKGROUND_DIMENSION {
        let scale = MAX_BACKGROUND_DIMENSION as f64 / (width.max(height) as f64);
        let new_width = (width as f64 * scale) as u32;
        let new_height = (height as f64 * scale) as u32;

        return Some(image::imageops::resize(
            &image,
            new_width,
            new_height,
            image::imageops::FilterType::Triangle,
        ));
    }

    Some(image)
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
            if let Ok(path) = uri.to_file_path() {
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }

    let path = PathBuf::from(trimmed);
    if path.is_file() {
        return Some(path);
    }

    None
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

fn build_background_gradient_preview_button(
    index: usize,
    density: BackgroundSidebarDensity,
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
    button.set_size_request(density.preview_button_size, density.preview_button_size);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class(density.preview_size_class);
    button.add_css_class(BACKGROUND_GRADIENT_PREVIEW_CLASSES[index]);
    button.set_tooltip_text(Some("Gradient"));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(density.preview_draw_size);
    preview_area.set_content_height(density.preview_draw_size);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    let preview_surface = Rc::new(RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let preview_surface_draw = preview_surface.clone();
    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = preview_surface_draw.borrow().as_ref() {
            draw_preview_tile_surface(
                context,
                surface,
                width,
                height,
                density.preview_corner_radius as f64,
            );
        }
    });

    button.set_child(Some(&preview_area));

    (button, preview_area, preview_surface)
}

fn build_background_wallpaper_preview_button(
    surface: Option<gtk4::cairo::ImageSurface>,
    tooltip: &str,
    density: BackgroundSidebarDensity,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(density.preview_button_size, density.preview_button_size);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class(density.preview_size_class);
    button.set_tooltip_text(Some(tooltip));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(density.preview_draw_size);
    preview_area.set_content_height(density.preview_draw_size);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = surface.as_ref() {
            draw_preview_tile_surface(
                context,
                surface,
                width,
                height,
                density.preview_corner_radius as f64,
            );
        }
    });

    button.set_child(Some(&preview_area));
    button
}

fn build_background_add_wallpaper_button(density: BackgroundSidebarDensity) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(density.preview_button_size, density.preview_button_size);
    button.add_css_class("editor-background-add-button");
    button.add_css_class(density.preview_size_class);
    button.set_tooltip_text(Some("Add wallpaper"));

    let plus_label = Label::new(Some("+"));
    plus_label.add_css_class("editor-background-add-label");
    plus_label.add_css_class(density.preview_size_class);
    button.set_child(Some(&plus_label));

    button
}

fn build_background_blurred_preview_button(
    index: usize,
    density: BackgroundSidebarDensity,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(density.preview_button_size, density.preview_button_size);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class(density.preview_size_class);
    button.add_css_class("editor-background-blurred-button");

    // Add intensity-specific class, label, and tooltip
    let (intensity_class, label, tooltip) = match index {
        0 => ("blur-light", "L", "Light Blur"),
        1 => ("blur-medium", "M", "Medium Blur"),
        2 => ("blur-heavy", "H", "Heavy Blur"),
        _ => ("", "", "Blurred"),
    };
    button.add_css_class(intensity_class);
    button.set_tooltip_text(Some(tooltip));

    let label_widget = Label::new(Some(label));
    label_widget.add_css_class("editor-blur-intensity-label");
    label_widget.set_halign(gtk4::Align::Center);
    label_widget.set_valign(gtk4::Align::Center);

    button.set_child(Some(&label_widget));
    button
}

fn clear_active_alignment_classes(widget: &gtk4::Widget) {
    if let Some(btn) = widget.downcast_ref::<Button>() {
        btn.remove_css_class("active-alignment-option");
    }

    let mut child = widget.first_child();
    while let Some(c) = child {
        clear_active_alignment_classes(&c);
        child = c.next_sibling();
    }
}

fn clear_active_background_classes(widget: &gtk4::Widget) {
    if let Some(btn) = widget.downcast_ref::<Button>() {
        btn.remove_css_class("active-background-option");
    }

    let mut child = widget.first_child();
    while let Some(c) = child {
        clear_active_background_classes(&c);
        child = c.next_sibling();
    }
}

fn rebuild_gradients_grid(
    gradients_grid: &GtkBox,
    all_previews: &[(
        Button,
        DrawingArea,
        Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>,
    )],
    collapsed: bool,
    density: BackgroundSidebarDensity,
) {
    while let Some(child) = gradients_grid.first_child() {
        gradients_grid.remove(&child);
    }

    let count = if collapsed { 10 } else { all_previews.len() };

    for chunk in all_previews[..count].chunks(BACKGROUND_PREVIEW_COLUMNS) {
        let gradient_row = GtkBox::new(Orientation::Horizontal, density.preview_row_spacing);
        gradient_row.add_css_class("editor-background-gradients-row");

        for (preview_button, _, _) in chunk {
            gradient_row.append(preview_button);
        }

        gradients_grid.append(&gradient_row);
    }
}

fn rebuild_wallpaper_preview_grid(
    wallpaper_grid: &GtkBox,
    wallpaper_previews: &[(String, Option<gtk4::cairo::ImageSurface>, PathBuf)],
    add_button: &Button,
    density: BackgroundSidebarDensity,
    state: Arc<Mutex<EditorState>>,
    sidebar: GtkBox,
    drawing_area: DrawingArea,
) {
    while let Some(child) = wallpaper_grid.first_child() {
        wallpaper_grid.remove(&child);
    }

    let new_row = || {
        let row = GtkBox::new(Orientation::Horizontal, density.preview_row_spacing);
        row.add_css_class("editor-background-wallpaper-row");
        row
    };

    let mut row = new_row();
    let mut items_in_row = 0usize;

    for (tooltip, surface, path) in wallpaper_previews {
        if items_in_row == BACKGROUND_PREVIEW_COLUMNS {
            wallpaper_grid.append(&row);
            row = new_row();
            items_in_row = 0;
        }

        let preview_button =
            build_background_wallpaper_preview_button(surface.clone(), tooltip, density);

        {
            let st = state.lock().unwrap();
            if let BackgroundStyle::Wallpaper(p) = &st.background_style {
                if p == path {
                    preview_button.add_css_class("active-background-option");
                }
            }
        }

        preview_button.connect_clicked({
            let state = state.clone();
            let sidebar = sidebar.clone();
            let btn = preview_button.clone();
            let path = path.clone();
            let drawing_area = drawing_area.clone();
            move |_| {
                println!("[DEBUG] Wallpaper tile clicked: {:?}", path);
                let mut st = state.lock().unwrap();
                st.background_style = BackgroundStyle::Wallpaper(path.clone());
                clear_active_background_classes(sidebar.upcast_ref());
                btn.add_css_class("active-background-option");
                st.mark_working_image_dirty();
                drawing_area.queue_draw();
            }
        });

        row.append(&preview_button);
        items_in_row += 1;
    }

    if items_in_row == BACKGROUND_PREVIEW_COLUMNS {
        wallpaper_grid.append(&row);
        row = new_row();
    }

    row.append(add_button);
    wallpaper_grid.append(&row);
}

pub(super) struct BackgroundPanelParts {
    pub root: GtkBox,
    pub start_gradient_preview_loading: Rc<dyn Fn()>,
}

pub(super) fn build_background_panel(
    window: &ApplicationWindow,
    state: Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    wallpaper_loader_sender: std::sync::mpsc::Sender<(Option<usize>, PathBuf, RgbaImage)>,
) -> BackgroundPanelParts {
    let density =
        BackgroundSidebarDensity::for_gradient_count(BACKGROUND_GRADIENT_PREVIEW_FILES.len());

    let left_column_group = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);
    let right_column_group = gtk4::SizeGroup::new(gtk4::SizeGroupMode::Horizontal);

    let background_sidebar = GtkBox::new(Orientation::Vertical, density.section_spacing);
    background_sidebar.add_css_class("editor-background-sidebar");
    background_sidebar.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    background_sidebar.set_hexpand(false);
    background_sidebar.set_visible(false);
    background_sidebar.set_vexpand(true);

    let background_sidebar_options = GtkBox::new(Orientation::Vertical, density.content_spacing);
    background_sidebar_options.add_css_class("editor-background-sidebar-options");

    let background_none_btn = Button::with_label("None");
    background_none_btn.set_has_frame(false);
    background_none_btn.set_halign(gtk4::Align::Fill);
    background_none_btn.set_hexpand(false);
    background_none_btn.set_size_request(density.none_button_width, -1);
    background_none_btn.add_css_class("editor-background-option-button");

    {
        let st = state.lock().unwrap();
        if st.background_style == BackgroundStyle::None {
            background_none_btn.add_css_class("active-background-option");
        }
    }

    background_none_btn.connect_clicked({
        let state = state.clone();
        let sidebar = background_sidebar.clone();
        let none_btn = background_none_btn.clone();
        let drawing_area = drawing_area.clone();
        move |_| {
            let mut st = state.lock().unwrap();
            st.background_style = BackgroundStyle::None;
            clear_active_background_classes(sidebar.upcast_ref());
            none_btn.add_css_class("active-background-option");
            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    let gradients_section = GtkBox::new(Orientation::Vertical, density.section_spacing);
    gradients_section.add_css_class("editor-background-gradients-section");

    let gradients_header = GtkBox::new(Orientation::Horizontal, 0);
    let gradients_title = Label::new(Some("Gradients"));
    gradients_title.add_css_class("editor-background-section-title");
    gradients_title.set_xalign(0.0);
    gradients_title.set_hexpand(true);

    let gradients_collapsed = Rc::new(Cell::new(true));
    let gradients_toggle_btn = Button::with_label("Show more");
    gradients_toggle_btn.set_has_frame(false);
    gradients_toggle_btn.add_css_class("editor-background-section-action-button");

    gradients_header.append(&gradients_title);
    gradients_header.append(&gradients_toggle_btn);

    let gradients_grid = GtkBox::new(Orientation::Vertical, density.preview_row_spacing);
    gradients_grid.add_css_class("editor-background-gradients-grid");

    let all_gradient_previews: Vec<(
        Button,
        DrawingArea,
        Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>,
    )> = BACKGROUND_GRADIENT_PREVIEW_FILES
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let (btn, area, surface) = build_background_gradient_preview_button(index, density);

            {
                let st = state.lock().unwrap();
                if let BackgroundStyle::Gradient(i) = st.background_style {
                    if i == index {
                        btn.add_css_class("active-background-option");
                    }
                }
            }

            btn.connect_clicked({
                let state = state.clone();
                let sidebar = background_sidebar.clone();
                let btn = btn.clone();
                let drawing_area = drawing_area.clone();
                move |_| {
                    let mut st = state.lock().unwrap();
                    st.background_style = BackgroundStyle::Gradient(index);
                    clear_active_background_classes(sidebar.upcast_ref());
                    btn.add_css_class("active-background-option");
                    st.mark_working_image_dirty();
                    drawing_area.queue_draw();
                }
            });

            (btn, area, surface)
        })
        .collect();

    rebuild_gradients_grid(
        &gradients_grid,
        &all_gradient_previews,
        gradients_collapsed.get(),
        density,
    );

    gradients_toggle_btn.connect_clicked({
        let gradients_collapsed = gradients_collapsed.clone();
        let gradients_grid = gradients_grid.clone();
        let all_gradient_previews = all_gradient_previews
            .iter()
            .map(|(b, d, s)| (b.clone(), d.clone(), s.clone()))
            .collect::<Vec<_>>();
        let gradients_toggle_btn = gradients_toggle_btn.clone();
        let window_weak = window.downgrade();
        move |_| {
            let is_collapsed = gradients_collapsed.get();
            gradients_collapsed.set(!is_collapsed);
            gradients_toggle_btn.set_label(if !is_collapsed {
                "Show more"
            } else {
                "Show less"
            });
            rebuild_gradients_grid(
                &gradients_grid,
                &all_gradient_previews,
                gradients_collapsed.get(),
                density,
            );

            if let Some(win) = window_weak.upgrade() {
                win.queue_resize();
            }
        }
    });

    let background_gradient_previews_started = Rc::new(Cell::new(false));
    let start_background_gradient_preview_loading: Rc<dyn Fn()> = Rc::new({
        let all_gradient_previews = all_gradient_previews
            .iter()
            .map(|(_, d, s)| (d.clone(), s.clone()))
            .collect::<Vec<_>>();
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

            let all_gradient_previews = all_gradient_previews.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || loop {
                match receiver.try_recv() {
                    Ok((index, preview_image)) => {
                        if let Some(surface) = rgba_image_to_surface(&preview_image) {
                            *all_gradient_previews[index].1.borrow_mut() = Some(surface);
                            all_gradient_previews[index].0.queue_draw();
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

    gradients_section.append(&gradients_header);
    gradients_section.append(&gradients_grid);

    let wallpaper_section = GtkBox::new(Orientation::Vertical, density.section_spacing);
    wallpaper_section.add_css_class("editor-background-wallpaper-section");

    let wallpaper_title = Label::new(Some("Wallpaper"));
    wallpaper_title.add_css_class("editor-background-section-title");
    wallpaper_title.set_xalign(0.0);

    let wallpaper_grid = GtkBox::new(Orientation::Vertical, density.preview_row_spacing);
    wallpaper_grid.add_css_class("editor-background-wallpaper-grid");

    let wallpaper_previews = Rc::new(RefCell::new(Vec::<(
        String,
        Option<gtk4::cairo::ImageSurface>,
        PathBuf,
    )>::new()));

    let wallpaper_path = detect_system_wallpaper_path()
        .unwrap_or_else(|| background_gradient_asset_path(BACKGROUND_GRADIENT_PREVIEW_FILES[0]));

    let surface = load_background_preview_image(&wallpaper_path, BACKGROUND_GRADIENT_PREVIEW_SIZE)
        .and_then(|img| rgba_image_to_surface(&img));
    let label = wallpaper_path
        .file_name()
        .and_then(|f| f.to_str())
        .map(|s| format!("Wallpaper: {s}"))
        .unwrap_or_else(|| "Wallpaper".to_string());
    wallpaper_previews
        .borrow_mut()
        .push((label, surface, wallpaper_path));

    let add_wallpaper_btn = build_background_add_wallpaper_button(density);

    {
        let previews = wallpaper_previews.borrow();
        rebuild_wallpaper_preview_grid(
            &wallpaper_grid,
            previews.as_slice(),
            &add_wallpaper_btn,
            density,
            state.clone(),
            background_sidebar.clone(),
            drawing_area.clone(),
        );
    }

    add_wallpaper_btn.connect_clicked({
        let state = state.clone();
        let wallpaper_previews = wallpaper_previews.clone();
        let wallpaper_grid = wallpaper_grid.clone();
        let add_wallpaper_btn = add_wallpaper_btn.clone();
        let background_sidebar = background_sidebar.clone();
        let drawing_area = drawing_area.clone();
        let wallpaper_loader_sender = wallpaper_loader_sender.clone();
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
                let state = state.clone();
                let background_sidebar = background_sidebar.clone();
                let drawing_area = drawing_area.clone();
                let wallpaper_loader_sender = wallpaper_loader_sender.clone();
                move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                // 1. Quick preview loading (as before, but optimized in next step if needed)
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
                                    wallpaper_previews.borrow_mut().push((
                                        label,
                                        Some(surface),
                                        path.clone(),
                                    ));
                                    let previews = wallpaper_previews.borrow();
                                    rebuild_wallpaper_preview_grid(
                                        &wallpaper_grid,
                                        previews.as_slice(),
                                        &add_wallpaper_btn,
                                        density,
                                        state.clone(),
                                        background_sidebar.clone(),
                                        drawing_area.clone(),
                                    );
                                }

                                // 2. Background load full image for cache
                                let path_cache = path.clone();
                                let sender = wallpaper_loader_sender.clone();
                                std::thread::spawn(move || {
                                    if let Some(rgba) = load_background_image_optimized(&path_cache)
                                    {
                                        let _ = sender.send((None, path_cache, rgba));
                                    }
                                });
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

    let blurred_section = GtkBox::new(Orientation::Vertical, density.section_spacing);
    blurred_section.add_css_class("editor-background-blurred-section");

    let blurred_title = Label::new(Some("Blurred"));
    blurred_title.add_css_class("editor-background-section-title");
    blurred_title.set_xalign(0.0);

    let blurred_row = GtkBox::new(Orientation::Horizontal, density.preview_row_spacing);
    blurred_row.add_css_class("editor-background-blurred-row");

    for index in 0..3 {
        let blurred_button = build_background_blurred_preview_button(index, density);

        {
            let st = state.lock().unwrap();
            if let BackgroundStyle::Blurred(i) = st.background_style {
                if i == index {
                    blurred_button.add_css_class("active-background-option");
                }
            }
        }

        blurred_button.connect_clicked({
            let state = state.clone();
            let sidebar = background_sidebar.clone();
            let btn = blurred_button.clone();
            let drawing_area = drawing_area.clone();
            move |_| {
                let mut st = state.lock().unwrap();
                st.background_style = BackgroundStyle::Blurred(index);
                clear_active_background_classes(sidebar.upcast_ref());
                btn.add_css_class("active-background-option");
                st.mark_working_image_dirty();
                drawing_area.queue_draw();
            }
        });
        blurred_row.append(&blurred_button);
    }

    blurred_section.append(&blurred_title);
    blurred_section.append(&blurred_row);

    let background_padding_divider_row = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider_row.add_css_class("editor-background-divider-row");
    let background_padding_divider = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider.add_css_class("editor-background-divider");
    background_padding_divider.set_size_request(density.divider_width, -1);
    background_padding_divider_row.append(&background_padding_divider);

    let padding_section = GtkBox::new(Orientation::Vertical, 4);
    padding_section.add_css_class("editor-background-padding-section");

    let padding_title = Label::new(Some("Padding"));
    padding_title.add_css_class("editor-background-section-title");
    padding_title.set_xalign(0.0);

    let padding_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    padding_slider.add_css_class("editor-opacity-slider");
    padding_slider.add_css_class("editor-background-padding-slider");
    padding_slider.set_draw_value(false);
    padding_slider.set_value(24.0);
    padding_slider.set_size_request(density.wide_slider_width, -1);
    padding_slider.set_halign(gtk4::Align::Fill);
    padding_slider.set_hexpand(false);
    padding_slider.set_margin_start(0);
    padding_slider.set_margin_end(0);

    let insert_slider_weak = Rc::new(RefCell::new(None::<Scale>));
    let padding_slider_weak = Rc::new(RefCell::new(None::<Scale>));
    let updating_sliders = Rc::new(Cell::new(false));

    padding_slider.connect_value_changed({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        let insert_slider_ref = insert_slider_weak.clone();
        let updating = updating_sliders.clone();
        move |s| {
            if updating.get() {
                return;
            }

            let mut st = state.lock().unwrap();
            st.background_padding = s.value();

            if st.auto_balance {
                updating.set(true);
                st.background_insert = s.value() * 0.8;
                if let Some(insert_s) = insert_slider_ref.borrow().as_ref() {
                    insert_s.set_value(st.background_insert);
                }
                updating.set(false);
            }

            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    padding_section.append(&padding_title);
    padding_section.append(&padding_slider);
    *padding_slider_weak.borrow_mut() = Some(padding_slider.clone());

    let compact_controls = GtkBox::new(Orientation::Vertical, 4);
    compact_controls.add_css_class("editor-background-compact-controls");

    let insert_shadow_row = GtkBox::new(Orientation::Horizontal, 8);
    insert_shadow_row.add_css_class("editor-background-compact-controls-row");
    insert_shadow_row.set_halign(gtk4::Align::Fill);
    insert_shadow_row.set_hexpand(true);

    let insert_section = GtkBox::new(Orientation::Vertical, 4);
    insert_section.add_css_class("editor-background-compact-slider-section");
    insert_section.set_size_request(density.compact_slider_width, -1);
    left_column_group.add_widget(&insert_section);

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
    insert_slider.set_halign(gtk4::Align::Fill);
    insert_slider.set_hexpand(true);
    insert_slider.set_margin_start(0);
    insert_slider.set_margin_end(0);
    insert_slider.connect_value_changed({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        let padding_slider_ref = padding_slider_weak.clone();
        let updating = updating_sliders.clone();
        move |s| {
            if updating.get() {
                return;
            }

            let mut st = state.lock().unwrap();
            st.background_insert = s.value();

            if st.auto_balance {
                updating.set(true);
                st.background_padding = s.value() / 0.8;
                if let Some(padding_s) = padding_slider_ref.borrow().as_ref() {
                    padding_s.set_value(st.background_padding);
                }
                updating.set(false);
            }

            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });
    *insert_slider_weak.borrow_mut() = Some(insert_slider.clone());

    insert_slider_row.append(&insert_slider);

    insert_section.append(&insert_title);
    insert_section.append(&insert_slider_row);

    let auto_balance_section = GtkBox::new(Orientation::Vertical, 2);
    auto_balance_section.add_css_class("editor-background-compact-slider-section");
    auto_balance_section.set_hexpand(true);
    auto_balance_section.set_halign(gtk4::Align::Fill);
    right_column_group.add_widget(&auto_balance_section);

    let auto_balance_title = Label::new(Some("Auto-balance"));
    auto_balance_title.add_css_class("editor-background-section-title");
    auto_balance_title.set_xalign(0.0);

    let auto_balance_check_row = GtkBox::new(Orientation::Horizontal, 0);
    auto_balance_check_row.add_css_class("editor-background-checkbox-row");
    let auto_balance_check = CheckButton::with_label("");
    auto_balance_check.add_css_class("editor-background-checkbox");
    auto_balance_check.set_halign(gtk4::Align::Start);
    {
        let st = state.lock().unwrap();
        auto_balance_check.set_active(st.auto_balance);
    }
    auto_balance_check_row.append(&auto_balance_check);

    auto_balance_check.connect_toggled({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        move |c| {
            let mut st = state.lock().unwrap();
            st.auto_balance = c.is_active();
            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    auto_balance_section.append(&auto_balance_title);
    auto_balance_section.append(&auto_balance_check_row);

    insert_shadow_row.append(&insert_section);
    insert_shadow_row.append(&auto_balance_section);

    let shadow_corners_row = GtkBox::new(Orientation::Horizontal, 8);
    shadow_corners_row.add_css_class("editor-background-compact-controls-row");
    shadow_corners_row.set_halign(gtk4::Align::Fill);
    shadow_corners_row.set_hexpand(true);

    let shadow_section = GtkBox::new(Orientation::Vertical, 4);
    shadow_section.add_css_class("editor-background-compact-slider-section");
    shadow_section.set_size_request(density.compact_slider_width, -1);
    left_column_group.add_widget(&shadow_section);

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
    shadow_slider.set_size_request(density.compact_slider_width, -1);
    shadow_slider.set_halign(gtk4::Align::Fill);
    shadow_slider.set_hexpand(true);
    shadow_slider.set_margin_start(0);
    shadow_slider.set_margin_end(0);
    shadow_slider.connect_value_changed({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        move |s| {
            let mut st = state.lock().unwrap();
            st.background_shadow = s.value();
            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    shadow_slider_row.append(&shadow_slider);

    shadow_section.append(&shadow_title);
    shadow_section.append(&shadow_slider_row);

    let alignment_section = GtkBox::new(Orientation::Vertical, 4);
    alignment_section.add_css_class("editor-background-compact-slider-section");
    alignment_section.set_size_request(density.none_button_width, -1);
    alignment_section.set_halign(gtk4::Align::Fill);
    alignment_section.set_hexpand(false);

    let alignment_title = Label::new(Some("Alignment"));
    alignment_title.add_css_class("editor-background-section-title");
    alignment_title.set_xalign(0.0);

    let alignment_grid = GtkBox::new(Orientation::Vertical, 4);
    alignment_grid.add_css_class("editor-background-alignment-grid");
    alignment_grid.set_size_request(density.none_button_width, -1);
    alignment_grid.set_halign(gtk4::Align::Fill);
    alignment_grid.set_hexpand(false);

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
        let alignment_row = GtkBox::new(Orientation::Horizontal, 4);
        alignment_row.add_css_class("editor-background-alignment-row");
        alignment_row.set_homogeneous(true);
        alignment_row.set_size_request(density.none_button_width, -1);
        alignment_row.set_halign(gtk4::Align::Fill);
        alignment_row.set_hexpand(false);

        for (position_class, tooltip) in row_items {
            let alignment_frame = GtkBox::new(Orientation::Horizontal, 0);
            alignment_frame.add_css_class("editor-background-alignment-icon-frame");
            alignment_frame.add_css_class(position_class);
            alignment_frame.set_size_request(12, 9);

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
            alignment_icon.set_size_request(34, 24);
            alignment_icon.set_child(Some(&alignment_frame));

            let alignment_button = Button::new();
            alignment_button.set_child(Some(&alignment_icon));
            alignment_button.set_has_frame(false);
            alignment_button.set_focusable(false);
            alignment_button.set_hexpand(true);
            alignment_button.set_tooltip_text(Some(tooltip));
            alignment_button.add_css_class("editor-background-alignment-button");
            let alignment_btn_style = match position_class {
                "top-left" => BackgroundAlignment::TopLeft,
                "top-center" => BackgroundAlignment::TopCenter,
                "top-right" => BackgroundAlignment::TopRight,
                "center-left" => BackgroundAlignment::CenterLeft,
                "center" => BackgroundAlignment::Center,
                "center-right" => BackgroundAlignment::CenterRight,
                "bottom-left" => BackgroundAlignment::BottomLeft,
                "bottom-center" => BackgroundAlignment::BottomCenter,
                "bottom-right" => BackgroundAlignment::BottomRight,
                _ => BackgroundAlignment::Center,
            };

            alignment_button.connect_clicked({
                let state = state.clone();
                let grid = alignment_grid.clone();
                let btn = alignment_button.clone();
                let drawing_area = drawing_area.clone();
                move |_| {
                    let mut st = state.lock().unwrap();
                    st.background_alignment = alignment_btn_style;
                    clear_active_alignment_classes(grid.upcast_ref());
                    btn.add_css_class("active-alignment-option");
                    st.mark_working_image_dirty();
                    drawing_area.queue_draw();
                }
            });

            {
                let st = state.lock().unwrap();
                if st.background_alignment == alignment_btn_style {
                    alignment_button.add_css_class("active-alignment-option");
                }
            }

            alignment_row.append(&alignment_button);
        }

        alignment_grid.append(&alignment_row);
    }

    alignment_section.append(&alignment_title);
    alignment_section.append(&alignment_grid);

    let corners_section = GtkBox::new(Orientation::Vertical, 4);
    corners_section.add_css_class("editor-background-compact-slider-section");
    corners_section.set_hexpand(true);
    corners_section.set_halign(gtk4::Align::Fill);
    right_column_group.add_widget(&corners_section);

    let ratio_section = GtkBox::new(Orientation::Vertical, 4);
    ratio_section.add_css_class("editor-background-compact-slider-section");
    ratio_section.set_size_request(density.wide_slider_width, -1);
    ratio_section.set_halign(gtk4::Align::Fill);
    ratio_section.set_hexpand(false);

    let ratio_title = Label::new(Some("Ratio"));
    ratio_title.add_css_class("editor-background-section-title");
    ratio_title.set_xalign(0.0);

    let ratio_dropdown_row = GtkBox::new(Orientation::Horizontal, 0);
    ratio_dropdown_row.add_css_class("editor-background-ratio-dropdown-row");
    ratio_dropdown_row.set_size_request(density.wide_slider_width, -1);
    ratio_dropdown_row.set_halign(gtk4::Align::Fill);
    ratio_dropdown_row.set_hexpand(false);
    let ratio_dropdown = gtk4::DropDown::from_strings(&["Original", "1:1", "4:3", "16:9", "21:9"]);
    ratio_dropdown.add_css_class("editor-background-ratio-dropdown");
    ratio_dropdown.set_size_request(density.wide_slider_width, -1);
    ratio_dropdown.set_halign(gtk4::Align::Fill);
    ratio_dropdown.set_hexpand(true);
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
    corners_slider.set_size_request(density.compact_slider_width, -1);
    corners_slider.set_halign(gtk4::Align::Fill);
    corners_slider.set_hexpand(true);
    corners_slider.set_margin_start(0);
    corners_slider.connect_value_changed({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        move |s| {
            let mut st = state.lock().unwrap();
            st.background_corner_radius = s.value();
            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    ratio_dropdown.connect_selected_item_notify({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        move |d| {
            let selected = d.selected();
            let aspect = match selected {
                0 => CropAspectRatio::Original,
                1 => CropAspectRatio::Square,
                2 => CropAspectRatio::FourThree,
                3 => CropAspectRatio::SixteenNine,
                4 => CropAspectRatio::TwentyOneNine,
                _ => CropAspectRatio::Original,
            };
            let mut st = state.lock().unwrap();
            st.background_aspect_ratio = aspect;
            st.mark_working_image_dirty();
            drawing_area.queue_draw();
        }
    });

    corners_slider_row.append(&corners_slider);

    corners_section.append(&corners_title);
    corners_section.append(&corners_slider_row);

    shadow_corners_row.append(&shadow_section);
    shadow_corners_row.append(&corners_section);

    compact_controls.append(&insert_shadow_row);
    compact_controls.append(&shadow_corners_row);

    background_sidebar_options.append(&background_none_btn);
    background_sidebar_options.append(&alignment_section);
    background_sidebar_options.append(&gradients_section);
    background_sidebar_options.append(&wallpaper_section);
    background_sidebar_options.append(&blurred_section);
    background_sidebar_options.append(&background_padding_divider_row);
    background_sidebar_options.append(&padding_section);
    background_sidebar_options.append(&ratio_section);
    background_sidebar_options.append(&compact_controls);

    background_sidebar.append(&background_sidebar_options);

    BackgroundPanelParts {
        root: background_sidebar,
        start_gradient_preview_loading: start_background_gradient_preview_loading,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn alignment_section_is_placed_below_none_button_and_before_gradients() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);

        let none_append = production_source
            .find("background_sidebar_options.append(&background_none_btn);")
            .expect("expected None button to be appended to background sidebar options");
        let alignment_append = production_source
            .find("background_sidebar_options.append(&alignment_section);")
            .expect("expected alignment section to be appended to background sidebar options");
        let gradients_append = production_source
            .find("background_sidebar_options.append(&gradients_section);")
            .expect("expected gradients section to be appended to background sidebar options");

        assert!(
            none_append < alignment_append && alignment_append < gradients_append,
            "alignment section should render between the None button and gradients section",
        );
        assert!(
            !production_source.contains("shadow_section.append(&alignment_section);"),
            "alignment section should no longer be nested under the shadow section",
        );
    }

    #[test]
    fn alignment_preview_widgets_use_larger_full_width_sizes() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production_source.contains("alignment_frame.set_size_request(12, 9);"),
            "alignment marker should use the enlarged preview marker size",
        );
        assert!(
            production_source.contains("alignment_icon.set_size_request(34, 24);"),
            "alignment icon should use the enlarged full-width button preview size",
        );
    }

    #[test]
    fn ratio_section_is_placed_below_padding_and_not_nested_under_corners() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);

        let padding_append = production_source
            .find("background_sidebar_options.append(&padding_section);")
            .expect("expected padding section to be appended to background sidebar options");
        let ratio_append = production_source
            .find("background_sidebar_options.append(&ratio_section);")
            .expect("expected ratio section to be appended to background sidebar options");
        let compact_append = production_source
            .find("background_sidebar_options.append(&compact_controls);")
            .expect("expected compact controls to be appended to background sidebar options");

        assert!(
            padding_append < ratio_append && ratio_append < compact_append,
            "ratio section should render between padding and compact controls",
        );
        assert!(
            !production_source.contains("corners_section.append(&ratio_section);"),
            "ratio section should no longer be nested under the corners section",
        );
    }

    #[test]
    fn background_panel_no_longer_appends_plain_color_section() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("plain_color_section.append(&plain_color_title);")
                && !production_source
                    .contains("background_sidebar_options.append(&plain_color_section);"),
            "Background panel should no longer render the embedded plain-color section",
        );
    }

    #[test]
    fn background_gradient_assets_support_installed_runtime_paths() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production_source.contains("/usr/share/apexshot/background-images")
                && production_source.contains("/usr/local/share/apexshot/background-images"),
            "background gradient lookup should support installed shared asset directories",
        );
    }
}
