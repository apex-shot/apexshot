//! A presentation-only slice of the image editor for the onboarding card.
//!
//! It is built from the production toolbar, the checkerboard canvas painter
//! and the shared Appearance panel, laid out exactly as the editor lays them
//! out. Only the left part of the window is shown: the card crops the editor
//! at its right and bottom edges, so the panel reads as a window that
//! continues past the card. No document, file I/O or account state is
//! attached.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::cairo::{Context, FontSlant, FontWeight, LinearGradient};
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{
    glib, ApplicationWindow, Box as GtkBox, DrawingArea, Orientation, Overlay, PolicyType,
    ScrolledWindow,
};

use super::canvas::draw_canvas_checkerboard;
use super::sections;
use super::toolbar::{self, Tool};
use super::ui_support::{self, px, INSPECTOR_WIDTH};
use crate::capture::editor::render::draw_arrow;
use crate::capture::editor::types::{ArrowStyle, DrawColor, Point};
use crate::settings::ui_support::TITLE_BAR_HEIGHT;

/// The editor window's layout size at 100% scale. The toolbar needs the
/// canvas to be about 590px wide, which fixes the width.
const WIDTH: f64 = 880.0;
const HEIGHT: f64 = 640.0;
/// How much of the layout is visible at the preview's zoom. The rest is
/// cropped by the card, so the inspector shows only its left edge.
const VISIBLE_WIDTH: f64 = 760.0;

/// The sample capture's own coordinate space.
const DOC_WIDTH: f64 = 640.0;
const DOC_HEIGHT: f64 = 440.0;

/// The brand orange the editor's accent controls use.
const ACCENT: (f64, f64, f64) = (1.0, 0.404, 0.051);

/// Reserved above the capture for the floating toolbar, below it for the
/// editor's own pills.
const CANVAS_TOP: f64 = 56.0;
const CANVAS_BOTTOM: f64 = 64.0;

/// The wallpaper the sample capture sits on, from the editor's own catalog.
const WALLPAPER: &str = "gradient-05";

mod viewport {
    use std::cell::{Cell, RefCell};

    use gtk4::subclass::prelude::*;
    use gtk4::{glib, gsk, prelude::*, Snapshot, Widget};

    #[derive(Default)]
    pub struct PreviewViewport {
        pub child: RefCell<Option<Widget>>,
        pub scale: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreviewViewport {
        const NAME: &'static str = "ApexShotOnboardingEditorPreview";
        type Type = super::PreviewViewport;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk4::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for PreviewViewport {
        fn dispose(&self) {
            if let Some(child) = self.child.borrow_mut().take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for PreviewViewport {
        // The editor keeps its own full-size allocation, but must not
        // propagate that minimum into the onboarding window's requisition.
        fn measure(&self, _: gtk4::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, _: i32, _: i32) {
            let Some(child) = self.child.borrow().as_ref().cloned() else {
                return;
            };
            let scale = self.scale.get();
            let zoom = super::preview_zoom(f64::from(width), scale);
            let layout_width = super::px(super::WIDTH, scale);
            let layout_height = super::px(super::HEIGHT, scale);
            child.measure(gtk4::Orientation::Horizontal, -1);
            child.measure(gtk4::Orientation::Vertical, layout_width);
            child.allocate(
                layout_width,
                layout_height,
                -1,
                Some(gsk::Transform::new().scale(zoom as f32, zoom as f32)),
            );
        }

        fn snapshot(&self, snapshot: &Snapshot) {
            if let Some(child) = self.child.borrow().as_ref() {
                self.obj().snapshot_child(child, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub struct PreviewViewport(ObjectSubclass<viewport::PreviewViewport>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

/// The zoom that fits the visible part of the editor into `width`. It depends
/// only on the ratio of the two, so the composition is the same at every
/// display scale.
fn preview_zoom(width: f64, scale: f64) -> f64 {
    width.max(0.0) / (VISIBLE_WIDTH * scale)
}

pub(crate) fn install_css() {
    ui_support::install_css();
}

pub(crate) fn build(scale: f64, dark: bool, window: &ApplicationWindow) -> gtk4::Widget {
    use gtk4::subclass::prelude::ObjectSubclassIsExt;

    let root = GtkBox::new(Orientation::Horizontal, 0);
    root.add_css_class("onboarding-editor-preview");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    root.set_overflow(gtk4::Overflow::Hidden);

    root.append(&build_canvas(scale, dark));
    root.append(&build_inspector(scale, window));

    let view: PreviewViewport = glib::Object::new();
    view.imp().scale.set(scale);
    root.set_parent(&view);
    view.imp().child.replace(Some(root.upcast()));
    view.set_hexpand(true);
    view.set_vexpand(true);
    view.set_overflow(gtk4::Overflow::Hidden);
    view.set_can_target(false);
    view.set_can_focus(false);
    view.update_property(&[gtk4::accessible::Property::Label(&crate::i18n::t(
        "Image editor preview with an annotated capture and the Appearance panel",
    ))]);
    view.upcast()
}

/// The checkerboard canvas with the floating tool rail and one capture.
fn build_canvas(scale: f64, dark: bool) -> Overlay {
    let canvas = Overlay::new();
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);

    let wallpaper = load_wallpaper();
    let art = DrawingArea::new();
    art.set_hexpand(true);
    art.set_vexpand(true);
    art.set_draw_func(move |_, cr, width, height| {
        draw_canvas_checkerboard(cr, f64::from(width), f64::from(height), scale, dark);
        draw_document(
            cr,
            f64::from(width),
            f64::from(height),
            scale,
            wallpaper.as_ref(),
        );
    });
    canvas.set_child(Some(&art));

    let tools = toolbar::build(scale, Tool::Arrow, Rc::new(|_| {}));
    canvas.add_overlay(&tools.root);
    canvas
}

/// The Appearance panel as the editor builds it, under the empty strip the
/// window controls occupy in the real editor.
fn build_inspector(scale: f64, window: &ApplicationWindow) -> GtkBox {
    let column = GtkBox::new(Orientation::Vertical, 0);
    column.add_css_class("image-editor-inspector");
    column.set_hexpand(false);
    column.set_size_request(px(INSPECTOR_WIDTH, scale), -1);

    let strip = GtkBox::new(Orientation::Horizontal, 0);
    strip.set_size_request(-1, px(TITLE_BAR_HEIGHT, scale));
    column.append(&strip);

    let (panel, _transform, _sync) = sections::appearance_panel(scale, None, &column, window);
    mark_sample_wallpaper(&panel);

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::External)
        .vscrollbar_policy(PolicyType::External)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .vexpand(true)
        .build();
    scroller.add_css_class("image-editor-inspector-scroll");
    scroller.set_child(Some(&panel));
    column.append(&scroller);
    column
}

fn collect_class(widget: &gtk4::Widget, class: &str, found: &mut Vec<gtk4::Widget>) {
    if widget.has_css_class(class) {
        found.push(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_class(&current, class, found);
        child = current.next_sibling();
    }
}

/// The capture sits on a catalog wallpaper, so the panel should show that
/// wallpaper as the chosen fill instead of "None". Without the catalog on
/// disk the panel stays as the editor opens it.
fn mark_sample_wallpaper(panel: &GtkBox) {
    let panel: &gtk4::Widget = panel.upcast_ref();
    let mut tiles = Vec::new();
    collect_class(panel, "image-editor-wallpaper-tile", &mut tiles);
    // Each tile's tooltip is its thumbnail's file stem.
    let thumbnail = WALLPAPER.replace("gradient-", "gradient-thumb-");
    let Some(tile) = tiles
        .iter()
        .find(|tile| tile.tooltip_text().is_some_and(|name| name == thumbnail))
    else {
        return;
    };
    let mut none_rows = Vec::new();
    collect_class(panel, "image-editor-bg-none-row", &mut none_rows);
    for row in none_rows {
        row.remove_css_class("active-background-option");
    }
    tile.add_css_class("active-background-option");
}

fn wallpaper_path() -> Option<PathBuf> {
    let directory = crate::video_editor::background::asset_directory()?;
    let path = directory.join(format!("{WALLPAPER}.jpg"));
    path.is_file().then_some(path)
}

fn load_wallpaper() -> Option<Pixbuf> {
    Pixbuf::from_file_at_scale(wallpaper_path()?, 960, -1, true).ok()
}

fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// A bar with rounded top corners, standing on `y + h`.
fn rounded_top(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    cr.new_sub_path();
    cr.move_to(x, y + h);
    cr.line_to(x, y + r);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.arc(x + w - r, y + r, r, 3.0 * FRAC_PI_2, TAU);
    cr.line_to(x + w, y + h);
    cr.close_path();
}

fn text(cr: &Context, value: &str, x: f64, y: f64, size: f64, bold: bool) {
    cr.select_font_face(
        "Inter",
        FontSlant::Normal,
        if bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        },
    );
    cr.set_font_size(size);
    cr.move_to(x, y);
    let _ = cr.show_text(value);
}

/// Place the sample capture on the canvas the way the editor fits a document:
/// centred, below the toolbar and clear of the bottom pills.
fn draw_document(cr: &Context, width: f64, height: f64, scale: f64, wallpaper: Option<&Pixbuf>) {
    let _ = cr.save();
    cr.scale(scale, scale);
    let (width, height) = (width / scale, height / scale);
    let aspect = DOC_WIDTH / DOC_HEIGHT;
    let room = height - CANVAS_TOP - CANVAS_BOTTOM;
    let doc_width = (width - 56.0).min(room * aspect).max(1.0);
    let doc_height = doc_width / aspect;
    cr.translate(
        (width - doc_width) / 2.0,
        CANVAS_TOP + (room - doc_height) / 2.0,
    );
    let unit = doc_width / DOC_WIDTH;
    cr.scale(unit, unit);

    draw_capture(cr, wallpaper);

    // The editor's hairline around the document.
    cr.set_line_width(1.0 / unit);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
    cr.rectangle(0.0, 0.0, DOC_WIDTH, DOC_HEIGHT);
    let _ = cr.stroke();
    let _ = cr.restore();
}

/// The document: a wallpaper, an application window with a soft shadow, and
/// one selected arrow annotation.
fn draw_capture(cr: &Context, wallpaper: Option<&Pixbuf>) {
    let _ = cr.save();
    cr.rectangle(0.0, 0.0, DOC_WIDTH, DOC_HEIGHT);
    cr.clip();
    match wallpaper {
        Some(pixbuf) => {
            let fit = (DOC_WIDTH / f64::from(pixbuf.width().max(1)))
                .max(DOC_HEIGHT / f64::from(pixbuf.height().max(1)));
            let _ = cr.save();
            cr.translate(
                (DOC_WIDTH - f64::from(pixbuf.width()) * fit) / 2.0,
                (DOC_HEIGHT - f64::from(pixbuf.height()) * fit) / 2.0,
            );
            cr.scale(fit, fit);
            cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
        None => {
            let gradient = LinearGradient::new(0.0, DOC_HEIGHT, DOC_WIDTH, 0.0);
            gradient.add_color_stop_rgb(0.0, 0.53, 0.28, 0.72);
            gradient.add_color_stop_rgb(0.55, 0.96, 0.18, 0.16);
            gradient.add_color_stop_rgb(1.0, 1.0, 0.82, 0.35);
            let _ = cr.set_source(&gradient);
            let _ = cr.paint();
        }
    }

    let (wx, wy, ww, wh, radius) = (56.0, 52.0, 528.0, 336.0, 14.0);
    for layer in 0..8 {
        let spread = f64::from(layer) * 3.0;
        rounded(
            cr,
            wx - spread,
            wy + 14.0 - spread,
            ww + spread * 2.0,
            wh + spread * 2.0,
            radius + spread,
        );
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.045);
        let _ = cr.fill();
    }

    let _ = cr.save();
    rounded(cr, wx, wy, ww, wh, radius);
    cr.set_source_rgb(0.106, 0.106, 0.114);
    let _ = cr.fill_preserve();
    cr.clip();
    cr.translate(wx, wy);
    draw_window_content(cr);
    let _ = cr.restore();

    let start = Point { x: 604.0, y: 356.0 };
    let end = Point { x: 507.0, y: 270.0 };
    let bend = Point { x: 606.0, y: 282.0 };
    draw_arrow(
        cr,
        start,
        end,
        DrawColor::new(1.0, 1.0, 1.0, 1.0),
        4.0,
        ArrowStyle::Curved,
        Some(vec![start, bend, end]),
        false,
    );
    for point in [start, end] {
        cr.arc(point.x, point.y, 5.0, 0.0, TAU);
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    }
    let _ = cr.restore();
}

/// A small analytics window, in the window's own 528x336 coordinates.
fn draw_window_content(cr: &Context) {
    cr.rectangle(0.0, 0.0, 112.0, 336.0);
    cr.set_source_rgb(0.082, 0.082, 0.090);
    let _ = cr.fill();

    rounded(cr, 16.0, 16.0, 16.0, 16.0, 5.0);
    cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    let _ = cr.fill();

    for (index, bar) in [58.0, 46.0, 64.0, 40.0, 52.0].into_iter().enumerate() {
        let y = 62.0 + f64::from(index as u32) * 28.0;
        if index == 0 {
            rounded(cr, 8.0, y - 8.0, 96.0, 24.0, 6.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.07);
            let _ = cr.fill();
        }
        rounded(cr, 18.0, y, bar, 8.0, 4.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, if index == 0 { 0.62 } else { 0.16 });
        let _ = cr.fill();
    }

    let left = 136.0;
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    text(cr, "Weekly overview", left, 40.0, 15.0, true);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    text(cr, "Mar 3 – Mar 9", left, 58.0, 10.0, false);

    for (index, (label, value)) in [
        ("Captures", "1,284"),
        ("Shared", "862"),
        ("Time saved", "14 h"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = left + f64::from(index as u32) * 126.0;
        rounded(cr, x, 76.0, 114.0, 62.0, 9.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.055);
        let _ = cr.fill();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
        text(cr, label, x + 12.0, 98.0, 9.0, false);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        text(cr, value, x + 12.0, 124.0, 18.0, true);
    }

    for (index, bar) in [40.0, 64.0, 52.0, 86.0, 70.0, 110.0, 90.0, 128.0, 102.0]
        .into_iter()
        .enumerate()
    {
        let x = left + f64::from(index as u32) * 40.0;
        rounded_top(cr, x, 312.0 - bar, 28.0, bar, 5.0);
        if index == 7 {
            cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        }
        let _ = cr.fill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_the_same_composition_across_display_scales() {
        let at_100 = preview_zoom(600.0, 1.0);
        for scale in [1.25, 1.5, 2.0] {
            assert!((preview_zoom(600.0 * scale, scale) - at_100).abs() < 0.0001);
        }
    }

    #[test]
    fn preview_never_demands_the_editors_minimum_window_size() {
        let _ = crate::test_support::with_gtk(|| {
            let app = gtk4::Application::new(
                Some("org.apexshot.test.preview"),
                gtk4::gio::ApplicationFlags::NON_UNIQUE,
            );
            app.register(None::<&gtk4::gio::Cancellable>).unwrap();
            let window = ApplicationWindow::new(&app);
            for dark in [true, false] {
                let view = build(1.0, dark, &window);
                assert_eq!(view.measure(Orientation::Horizontal, -1).0, 0);
                assert_eq!(view.measure(Orientation::Vertical, -1).0, 0);
                assert!(!view.can_target());
                assert!(!view.can_focus());
                assert_eq!(view.accessible_role(), gtk4::AccessibleRole::Img);
                let root = view.first_child().unwrap();
                assert!(root.has_css_class("onboarding-editor-preview"));
                assert!(root.has_css_class(if dark {
                    "editor-theme-dark"
                } else {
                    "editor-theme-light"
                }));
            }
        });
    }

    #[test]
    fn the_inspector_keeps_its_dock_width_beside_the_canvas() {
        let _ = crate::test_support::with_gtk(|| {
            let app = gtk4::Application::new(
                Some("org.apexshot.test.preview-dock"),
                gtk4::gio::ApplicationFlags::NON_UNIQUE,
            );
            app.register(None::<&gtk4::gio::Cancellable>).unwrap();
            let window = ApplicationWindow::new(&app);
            let inspector = build_inspector(1.0, &window);
            assert!(!inspector.hexpands());
            assert_eq!(
                inspector.measure(Orientation::Horizontal, -1).0,
                px(INSPECTOR_WIDTH, 1.0)
            );
        });
    }

    #[test]
    fn the_sample_capture_renders_without_the_wallpaper_catalog() {
        let surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 592, 640).unwrap();
        let cr = Context::new(&surface).unwrap();
        draw_document(&cr, 592.0, 640.0, 1.0, None);
        cr.status().unwrap();
        surface.status().unwrap();
    }
}
