//! Background fill for the video editor's stage and its Wallpaper tab.
//!
//! The catalog is the reference editor's sixty bundled wallpapers. The
//! thumbnails and full-size images are read from the shared asset directory,
//! so both platforms show the same pictures from the same files; only the
//! list lives here.

use gtk4::cairo::Context;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

/// The Wallpaper tab's catalog, in the reference editor's order.
pub(super) const WALLPAPERS: [&str; 60] = [
    "wallpaper-001.jpg",
    "wallpaper-002.jpg",
    "wallpaper-003.jpg",
    "wallpaper-004.jpg",
    "wallpaper-005.jpg",
    "wallpaper-006.jpg",
    "wallpaper-007.jpg",
    "wallpaper-008.jpg",
    "wallpaper-009.jpg",
    "wallpaper-010.jpg",
    "wallpaper-011.jpg",
    "wallpaper-012.jpg",
    "wallpaper-013.jpg",
    "wallpaper-014.jpg",
    "wallpaper-015.jpg",
    "wallpaper-016.jpg",
    "wallpaper-017.jpg",
    "wallpaper-018.jpg",
    "wallpaper-019.jpg",
    "wallpaper-020.jpg",
    "wallpaper-021.jpg",
    "wallpaper-022.jpg",
    "wallpaper-023.jpg",
    "wallpaper-024.jpg",
    "wallpaper-025.jpg",
    "wallpaper-026.jpg",
    "wallpaper-027.jpg",
    "wallpaper-028.jpg",
    "wallpaper-029.jpg",
    "wallpaper-030.jpg",
    "wallpaper-031.jpg",
    "wallpaper-032.jpg",
    "wallpaper-033.jpg",
    "wallpaper-034.jpg",
    "wallpaper-035.jpg",
    "wallpaper-036.jpg",
    "wallpaper-037.jpg",
    "wallpaper-038.jpg",
    "wallpaper-039.jpg",
    "wallpaper-040.jpg",
    "wallpaper-041.jpg",
    "wallpaper-042.jpg",
    "wallpaper-043.jpg",
    "wallpaper-044.jpg",
    "wallpaper-045.jpg",
    "wallpaper-046.jpg",
    "wallpaper-047.jpg",
    "wallpaper-048.jpg",
    "wallpaper-049.jpg",
    "wallpaper-050.jpg",
    "wallpaper-051.jpg",
    "wallpaper-052.jpg",
    "wallpaper-053.jpg",
    "wallpaper-054.jpg",
    "wallpaper-055.jpg",
    "wallpaper-056.jpg",
    "wallpaper-057.jpg",
    "wallpaper-058.jpg",
    "wallpaper-059.jpg",
    "wallpaper-060.jpg",
];

/// Padding is expressed against a 400px long edge, so an inset scales with
/// the canvas exactly as the reference editor's export does.
pub(super) const PADDING_REFERENCE: f64 = 400.0;
/// The panel's Padding row starts here, and the stage insets by it whenever a
/// fill is active.
pub(super) const DEFAULT_PADDING: f64 = 24.0;
/// The panel's Radius row starts square, exactly as the reference editor's
/// state does: a non-zero default would round the corners of every video
/// without the user asking. Like padding it is expressed against a 400px long
/// edge, and unlike padding it stays live with no fill picked.
pub(super) const DEFAULT_RADIUS: f64 = 0.0;
/// Smallest surround kept between the video and the fill's edges, in padding
/// slider units. A frame with a fill would otherwise let the video touch the
/// background on one axis at padding 0, so rendering floors tiny paddings to
/// this breathing room. The slider keeps the user's value; only the stage and
/// the export use the floored one.
pub(super) const MIN_PADDING: f64 = 8.0;

/// Rendering padding for a canvas: the user's value, floored to
/// [`MIN_PADDING`] while a fill is active. Without a fill the video is the
/// canvas, so the inset is zero.
pub(super) fn effective_padding(padding: f64) -> f64 {
    if padding.is_finite() {
        padding.max(MIN_PADDING)
    } else {
        MIN_PADDING
    }
}

/// The fill chosen in the Background panel. Shared by the panel, which sets
/// it, and the stage, which paints it behind the video.
#[derive(Clone)]
pub(super) struct Background {
    selection: Rc<RefCell<Option<String>>>,
    padding: Rc<Cell<f64>>,
    radius: Rc<Cell<f64>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            selection: Rc::new(RefCell::new(None)),
            padding: Rc::new(Cell::new(DEFAULT_PADDING)),
            radius: Rc::new(Cell::new(DEFAULT_RADIUS)),
            listeners: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl Background {
    /// A read handle to the same state, without the listener list. Views hold
    /// this so a listener can inspect the fill without keeping the model
    /// alive through a reference cycle.
    pub fn state(&self) -> BackgroundState {
        BackgroundState {
            selection: self.selection.clone(),
            padding: self.padding.clone(),
            radius: self.radius.clone(),
        }
    }

    pub fn select(&self, file_name: Option<&str>) {
        let next = file_name.map(str::to_owned);
        let current = self.selection.borrow().clone();
        if current == next {
            return;
        }
        *self.selection.borrow_mut() = next;
        self.notify();
    }

    pub fn padding(&self) -> f64 {
        self.padding.get()
    }

    pub fn set_padding(&self, value: f64) {
        let value = value.max(0.0);
        if (self.padding.get() - value).abs() < f64::EPSILON {
            return;
        }
        self.padding.set(value);
        self.notify();
    }

    pub fn radius(&self) -> f64 {
        self.radius.get()
    }

    pub fn set_radius(&self, value: f64) {
        let value = value.max(0.0);
        if (self.radius.get() - value).abs() < f64::EPSILON {
            return;
        }
        self.radius.set(value);
        self.notify();
    }

    pub fn connect(&self, listener: Rc<dyn Fn()>) {
        self.listeners.borrow_mut().push(listener);
        self.notify();
    }

    fn notify(&self) {
        for listener in self.listeners.borrow().iter() {
            listener();
        }
    }
}

/// Read-only view of the fill, its padding, and the video card's corner
/// radius, shared with the stage.
#[derive(Clone)]
pub(super) struct BackgroundState {
    selection: Rc<RefCell<Option<String>>>,
    padding: Rc<Cell<f64>>,
    radius: Rc<Cell<f64>>,
}

impl BackgroundState {
    pub fn selection(&self) -> Option<String> {
        self.selection.borrow().clone()
    }

    /// True while a fill is picked. The stage insets and grows the canvas only
    /// then, so an empty background leaves the video edge to edge.
    pub fn has_fill(&self) -> bool {
        self.selection.borrow().is_some()
    }

    pub fn padding(&self) -> f64 {
        self.padding.get()
    }

    pub fn radius(&self) -> f64 {
        self.radius.get()
    }
}

/// Where the shared background images live: the repository during
/// development, then the installed copies next to the executable.
fn asset_directory() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(assets) = std::env::var_os("APEXSHOT_ASSETS") {
        roots.push(PathBuf::from(assets).join("background-images"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("src/capture/editor/background-images"));
        roots.push(cwd.join("../src/capture/editor/background-images"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            roots.push(directory.join("background-images"));
            roots.push(directory.join("../share/apexshot/background-images"));
        }
    }
    roots.into_iter().find(|root| root.is_dir())
}

fn asset_path(file_name: &str) -> Option<PathBuf> {
    let root = asset_directory()?;
    let path = root.join(file_name);
    path.is_file().then_some(path)
}

pub(super) fn wallpaper_path(file_name: &str) -> Option<PathBuf> {
    asset_path(file_name)
}

/// The small preview the grid draws instead of decoding a full-size image.
pub(super) fn thumb_path(file_name: &str) -> Option<PathBuf> {
    asset_path(&thumb_file_name(file_name)?)
}

fn thumb_file_name(file_name: &str) -> Option<String> {
    let suffix = file_name.strip_prefix("wallpaper-")?;
    Some(format!("wallpaper-thumb-{suffix}"))
}

/// The full-size wallpaper as a pixbuf, for the stage behind the video.
pub(super) fn load_pixbuf(file_name: &str) -> Option<Pixbuf> {
    let path = wallpaper_path(file_name)?;
    Pixbuf::from_file(&path).ok()
}

/// Paint a pixbuf over the whole surface, cropped to `cover` fit so a
/// landscape wallpaper fills a portrait canvas without stretching.
pub(super) fn paint_cover(context: &Context, width: i32, height: i32, pixbuf: &Pixbuf) {
    let width = f64::from(width.max(1));
    let height = f64::from(height.max(1));
    let source_width = f64::from(pixbuf.width().max(1));
    let source_height = f64::from(pixbuf.height().max(1));
    let scale = (width / source_width).max(height / source_height);
    let draw_width = source_width * scale;
    let draw_height = source_height * scale;
    let _ = context.save();
    context.rectangle(0.0, 0.0, width, height);
    context.clip();
    context.translate((width - draw_width) * 0.5, (height - draw_height) * 0.5);
    context.scale(scale, scale);
    context.set_source_pixbuf(pixbuf, 0.0, 0.0);
    let _ = context.paint();
    let _ = context.restore();
}

#[cfg(test)]
mod tests {
    use super::{
        thumb_file_name, thumb_path, Background, DEFAULT_PADDING, DEFAULT_RADIUS, WALLPAPERS,
    };

    #[test]
    fn the_catalog_matches_the_reference_editor() {
        assert_eq!(WALLPAPERS.len(), 60);
        assert_eq!(WALLPAPERS[0], "wallpaper-001.jpg");
        assert_eq!(WALLPAPERS[59], "wallpaper-060.jpg");
    }

    #[test]
    fn thumbnails_map_to_the_bundled_previews() {
        assert_eq!(
            thumb_file_name("wallpaper-001.jpg").as_deref(),
            Some("wallpaper-thumb-001.jpg")
        );
        assert_eq!(
            thumb_file_name("wallpaper-060.jpg").as_deref(),
            Some("wallpaper-thumb-060.jpg")
        );
        assert_eq!(thumb_file_name("gradient-01.jpg"), None);
    }

    #[test]
    fn the_repository_ships_every_wallpaper_and_thumbnail() {
        for file_name in WALLPAPERS {
            assert!(
                super::wallpaper_path(file_name).is_some(),
                "missing full-size wallpaper {file_name}"
            );
            assert!(
                thumb_path(file_name).is_some(),
                "missing thumbnail for {file_name}"
            );
        }
    }

    #[test]
    fn selecting_a_wallpaper_notifies_listeners_once() {
        let background = Background::default();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = calls.clone();
        background.connect(std::rc::Rc::new(move || counter.set(counter.get() + 1)));
        // `connect` paints once, then a real change paints again, and picking
        // the same wallpaper is a no-op.
        assert_eq!(calls.get(), 1);
        background.select(Some("wallpaper-004.jpg"));
        assert_eq!(calls.get(), 2);
        background.select(Some("wallpaper-004.jpg"));
        assert_eq!(calls.get(), 2);
        assert_eq!(
            background.state().selection().as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert_eq!(background.padding(), DEFAULT_PADDING);
        assert_eq!(background.radius(), DEFAULT_RADIUS);
    }

    #[test]
    fn the_radius_stays_live_without_a_fill() {
        let background = Background::default();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = calls.clone();
        background.connect(std::rc::Rc::new(move || counter.set(counter.get() + 1)));
        background.set_radius(20.0);
        assert_eq!(calls.get(), 2);
        assert!(background.state().selection().is_none());
        assert_eq!(background.state().radius(), 20.0);
        // A drag that repeats the same value must not repaint.
        background.set_radius(20.0);
        assert_eq!(calls.get(), 2);
    }
}
