//! The right-hand inspector.
//!
//! The Linux editor docks the shared Appearance panel here: an Appearance
//! title over one Background card. The card's Wallpapers / Custom / Image tabs
//! pick the fill source, then come the shared Padding and Border Radius
//! sliders. The palette switcher belongs to the Colors tool, not this panel.
//! Every control is a shape-only shell for now — nothing a drawing backend
//! could read exists on this platform yet.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, Grid, Image, Label, Orientation, PolicyType,
    ScrolledWindow, Stack, ToggleButton,
};

use super::toolbar::Tool;
use super::ui_support::{px, INSPECTOR_WIDTH};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::video_editor::side_panel::FilledSlider;

/// Edge of one wallpaper thumbnail. The tile is pinned to it and centred in
/// its grid cell, exactly as the Linux panel pins its tiles: a homogeneous
/// column otherwise stretches the preview and the grid stops reading as a
/// catalog.
const TILE_EDGE: f64 = 56.0;
/// Height of the catalog's own viewport. The grid scrolls inside it, so the
/// shared Padding and Radius rows below stay in reach.
const CATALOG_HEIGHT: f64 = 260.0;
const GRID_GAP: f64 = 8.0;
const WALLPAPER_COLUMNS: usize = 4;
const TILE_RADIUS: f64 = 11.0;

pub(super) struct Inspector {
    pub root: GtkBox,
}

/// The bundled wallpaper catalog sits in the shared Linux asset tree; the
/// Windows build reads it straight from the source checkout for now.
fn background_images_dir() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("../src/capture/editor/background-images")];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("src/capture/editor/background-images"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("background-images"));
        }
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn wallpaper_thumbs() -> Vec<PathBuf> {
    let Some(dir) = background_images_dir() else {
        return Vec::new();
    };
    let mut thumbs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("wallpaper-thumb-")
                                || name.starts_with("gradient-thumb-")
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    thumbs.sort();
    thumbs
}

fn rounded_rect(context: &gtk4::cairo::Context, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    context.new_sub_path();
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
}

/// Cover-fit the thumbnail into the tile, clipped to the tile's rounded
/// corners, the way the Linux panel paints its cairo previews.
fn paint_thumbnail(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    thumbnail: &Pixbuf,
    radius: f64,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    if width < 1.0 || height < 1.0 {
        return;
    }
    rounded_rect(context, width, height, radius);
    context.clip();
    let source_width = f64::from(thumbnail.width().max(1));
    let source_height = f64::from(thumbnail.height().max(1));
    let cover = (width / source_width).max(height / source_height);
    let draw_width = source_width * cover;
    let draw_height = source_height * cover;
    context.translate((width - draw_width) * 0.5, (height - draw_height) * 0.5);
    context.scale(cover, cover);
    context.set_source_pixbuf(thumbnail, 0.0, 0.0);
    let _ = context.paint();
}

/// The fill source the panel currently shows as chosen. Every tile and the
/// None row read from one place, so a wallpaper and None can never both carry
/// the ring.
#[derive(Clone)]
struct Selection {
    tiles: Rc<RefCell<Vec<Button>>>,
    none: Button,
}

impl Selection {
    fn clear(&self) {
        for tile in self.tiles.borrow().iter() {
            tile.remove_css_class("active-background-option");
        }
        self.none.add_css_class("active-background-option");
    }

    fn select(&self, tile: &Button) {
        for other in self.tiles.borrow().iter() {
            other.remove_css_class("active-background-option");
        }
        tile.add_css_class("active-background-option");
        self.none.remove_css_class("active-background-option");
    }
}

/// One wallpaper thumbnail tile: a fixed square preview whose ring is owned by
/// the panel's single selection rule, like the Linux grid.
fn wallpaper_tile(scale: f64, path: &PathBuf, selection: &Selection) -> Button {
    let edge = px(TILE_EDGE, scale);
    let preview = DrawingArea::new();
    preview.set_content_width(edge);
    preview.set_content_height(edge);
    preview.set_size_request(edge, edge);
    preview.set_can_target(false);
    // Decoding the whole catalog up front would stall the window; each preview
    // fills in on the next idle instead.
    {
        let preview = preview.clone();
        let path = path.clone();
        gtk4::glib::idle_add_local_once(move || {
            let Some(pixbuf) = Pixbuf::from_file_at_scale(&path, edge * 2, edge * 2, true).ok()
            else {
                return;
            };
            let radius = TILE_RADIUS * scale;
            preview.set_draw_func(move |_, context, width, height| {
                paint_thumbnail(context, width, height, &pixbuf, radius);
            });
            preview.queue_draw();
        });
    }

    let tile = Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
        .build();
    tile.add_css_class("image-editor-wallpaper-tile");
    tile.set_size_request(edge, edge);
    tile.set_hexpand(false);
    tile.set_halign(Align::Center);
    tile.set_valign(Align::Start);
    tile.set_child(Some(&preview));
    tile.connect_clicked({
        let selection = selection.clone();
        let tile = tile.clone();
        move |_| selection.select(&tile)
    });
    tile
}

/// The Wallpapers page: the catalog in its own fixed-height viewport, four
/// pinned tiles per row.
fn build_wallpaper_page(scale: f64, selection: &Selection) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);

    let grid = Grid::new();
    grid.add_css_class("image-editor-wallpaper-grid");
    // The tile natural sizes stay the grid's: without this a homogeneous
    // column stretches every thumbnail to the panel's width.
    grid.set_hexpand(false);
    grid.set_halign(Align::Fill);
    grid.set_column_homogeneous(true);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        grid.set_column_spacing(px(GRID_GAP, scale).max(0) as u32);
        grid.set_row_spacing(px(GRID_GAP, scale).max(0) as u32);
    }

    for (index, thumb) in wallpaper_thumbs().into_iter().enumerate() {
        let tile = wallpaper_tile(scale, &thumb, selection);
        selection.tiles.borrow_mut().push(tile.clone());
        #[allow(clippy::cast_possible_wrap)]
        grid.attach(
            &tile,
            (index % WALLPAPER_COLUMNS) as i32,
            (index / WALLPAPER_COLUMNS) as i32,
            1,
            1,
        );
    }

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .height_request(px(CATALOG_HEIGHT, scale))
        .child(&grid)
        .build();
    scroller.add_css_class("image-editor-wallpaper-scroll");
    page.append(&scroller);
    page
}

/// The Custom page: the current fill summarized by its swatch, its kind and a
/// trailing Edit pill. The editors behind it live in the video editor's Custom
/// Wallpaper popover on Linux, so this row only ever summarizes.
fn build_custom_page(scale: f64) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.set_hexpand(true);

    let row = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
    row.add_css_class("image-editor-bg-custom-row");
    row.set_hexpand(true);

    let swatch = GtkBox::new(Orientation::Horizontal, 0);
    swatch.set_size_request(px(28.0, scale), px(28.0, scale));
    swatch.set_valign(Align::Center);
    swatch.add_css_class("image-editor-bg-custom-swatch");

    let kind = Label::new(Some(&t("Color")));
    kind.set_xalign(0.0);
    kind.set_hexpand(true);
    kind.set_valign(Align::Center);
    kind.add_css_class("image-editor-bg-custom-kind");

    let edit = Button::with_label(&t("Edit"));
    edit.add_css_class("image-editor-bg-custom-edit");
    edit.set_has_frame(false);
    edit.set_valign(Align::Center);
    edit.set_sensitive(false);
    edit.set_tooltip_text(Some(&t("Edit custom fill")));

    row.append(&swatch);
    row.append(&kind);
    row.append(&edit);
    page.append(&row);
    page
}

/// The Image page: one full-width chooser row carrying a preview chip, the
/// picker label and the folder affordance.
fn build_image_page(scale: f64) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, px(6.0, scale));
    page.set_hexpand(true);

    let row = Button::new();
    row.add_css_class("image-editor-bg-image-row");
    row.set_has_frame(false);
    row.set_hexpand(true);
    row.set_sensitive(false);
    row.set_tooltip_text(Some(&t(
        "Background images are not available in the Windows image editor yet",
    )));

    let inner = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
    let thumb = GtkBox::new(Orientation::Horizontal, 0);
    thumb.set_size_request(px(28.0, scale), px(28.0, scale));
    thumb.set_valign(Align::Center);
    thumb.add_css_class("image-editor-bg-custom-swatch");
    thumb.add_css_class("image-editor-bg-image-thumb");
    let label = Label::new(Some(&t("Select image...")));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    icon.set_pixel_size(px(13.0, scale));
    icon.set_valign(Align::Center);
    icon.add_css_class("image-editor-bg-image-icon");
    inner.append(&thumb);
    inner.append(&label);
    inner.append(&icon);
    row.set_child(Some(&inner));
    page.append(&row);
    page
}

/// The Appearance panel's section card: a heading over the rows it owns, the
/// same raised surface the Linux sidebar gives every tool group.
fn appearance_section(title: &str, scale: f64) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    section.add_css_class("image-editor-section");
    let heading = Label::new(Some(&t(title)));
    heading.add_css_class("image-editor-section-title");
    heading.set_xalign(0.0);
    section.append(&heading);
    section
}

/// One pill in the fill-source tray, reusing the video editor's Background tab
/// look so the two panels read identically.
fn source_tab(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(&t(label));
    button.add_css_class("image-editor-bg-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn build_source_pages(scale: f64, selection: &Selection) -> Stack {
    let pages = Stack::new();
    pages.set_hhomogeneous(false);
    pages.set_vhomogeneous(false);
    pages.set_hexpand(true);
    pages.add_named(&build_wallpaper_page(scale, selection), Some("wallpapers"));
    pages.add_named(&build_custom_page(scale), Some("custom"));
    pages.add_named(&build_image_page(scale), Some("image"));
    pages.set_visible_child_name("wallpapers");
    pages
}

pub(super) fn build(scale: f64) -> Inspector {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("image-editor-inspector");
    root.set_hexpand(false);
    root.set_size_request(px(INSPECTOR_WIDTH, scale), -1);

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .overlay_scrolling(true)
        .vexpand(true)
        .build();
    scroller.add_css_class("image-editor-inspector-scroll");
    // The wallpaper catalog reports a big natural size; keep the panel at its
    // fixed width and let the content scroll inside.
    scroller.set_propagate_natural_width(false);
    scroller.set_propagate_natural_height(false);
    scroller.set_hexpand(false);

    let body = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    body.add_css_class("image-editor-inspector-body");

    let title = Label::new(Some(&t("Appearance")));
    title.add_css_class("image-editor-inspector-title");
    title.set_xalign(0.0);
    body.append(&title);

    let section = appearance_section("Background", scale);

    // "None" clears the fill outright. It is not a fourth source tab, so it
    // sits under the tray on every tab rather than inside it, and it carries
    // the ring exactly while no tile is chosen.
    let none_row = Button::with_label(&t("None"));
    none_row.add_css_class("image-editor-bg-none-row");
    none_row.set_has_frame(false);
    none_row.set_hexpand(true);
    none_row.set_halign(Align::Fill);
    none_row.add_css_class("active-background-option");
    let selection = Selection {
        tiles: Rc::new(RefCell::new(Vec::new())),
        none: none_row.clone(),
    };
    none_row.connect_clicked({
        let selection = selection.clone();
        move |_| selection.clear()
    });

    // Fill-source tabs: one source at a time, matching the Linux panel.
    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("image-editor-bg-tabs");
    tabs.set_hexpand(true);
    tabs.set_homogeneous(true);
    let pages = build_source_pages(scale, &selection);
    let mut first_tab: Option<ToggleButton> = None;
    for (label, page_name, active) in [
        ("Wallpapers", "wallpapers", true),
        ("Custom", "custom", false),
        ("Image", "image", false),
    ] {
        let tab = source_tab(label);
        if let Some(first) = &first_tab {
            tab.set_group(Some(first));
        } else {
            first_tab = Some(tab.clone());
        }
        tab.set_active(active);
        tab.connect_toggled({
            let pages = pages.clone();
            move |button| {
                if button.is_active() {
                    pages.set_visible_child_name(page_name);
                }
            }
        });
        tabs.append(&tab);
    }
    section.append(&tabs);
    section.append(&none_row);
    section.append(&pages);

    // Padding and Radius are shared by every source tab: they sit inside the
    // card, right under the open page.
    let options = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    options.add_css_class("image-editor-bg-options");
    options.append(&FilledSlider::new(&t("Padding"), 0.0, 80.0, 40.0, 1.0, 0, scale).widget());
    options.append(&FilledSlider::new(&t("Border Radius"), 0.0, 40.0, 0.0, 1.0, 0, scale).widget());
    section.append(&options);

    body.append(&section);
    scroller.set_child(Some(&body));
    root.append(&scroller);

    // Bottom actions: copy and upload are icon buttons on the left, Export
    // owns the right edge as the panel's one accent pill.
    let actions = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    actions.add_css_class("image-editor-actions");
    actions.append(&action_icon_button(
        scale,
        icon_names::custom::COPY_SYMBOLIC,
        &t("Copy to clipboard"),
    ));
    actions.append(&action_icon_button(
        scale,
        icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
        &t("Upload to cloud"),
    ));
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    let export = Button::with_label(&t("Export"));
    export.add_css_class("image-editor-export");
    export.set_has_frame(false);
    export.set_valign(Align::Center);
    export.set_sensitive(false);
    export.set_tooltip_text(Some(&t(
        "Export is not available in the Windows image editor yet",
    )));
    actions.append(&export);
    root.append(&actions);

    Inspector { root }
}

/// A flat 18px icon button, the shape the Linux sidebar gives Copy and Upload.
fn action_icon_button(scale: f64, icon: &str, tooltip: &str) -> Button {
    let image = Image::from_icon_name(icon);
    image.set_pixel_size(px(18.0, scale));
    image.add_css_class("image-editor-action-icon-glyph");
    let button = Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .child(&image)
        .sensitive(false)
        .build();
    button.add_css_class("image-editor-action-icon");
    button
}

impl Inspector {
    /// The Appearance panel is the permanent dock for now; tool-specific pages
    /// land with their drawing backends.
    pub(super) fn show_tool(&self, _tool: Tool) {}
}
