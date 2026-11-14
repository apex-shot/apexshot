//! Background fill for the video editor's stage and its Wallpaper tab.
//!
//! The catalog is the reference editor's sixty bundled wallpapers. The
//! thumbnails and full-size images are read from the shared asset directory,
//! so both platforms show the same pictures from the same files; only the
//! list lives here.

use gtk4::cairo::Context;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
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

/// A saved stage metric: negatives have no meaning on a canvas, so they floor
/// at zero, and a value that is not a number reads as the default it started
/// from.
fn stage_metric(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        default
    }
}

/// A fill the panel paints itself, drawn over the whole canvas. The Custom tab
/// owns the color and gradient model, so it hands the stage a painter instead of
/// its state: the stage needs pixels, not a fill type.
pub(crate) type FillPainter = Rc<dyn Fn(&Context, i32, i32)>;

/// The shape a gradient fill can take, kept beside the fill so the project file
/// can name it without the panel's widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GradientKind {
    Linear,
    Radial,
    Angular,
    Diamond,
}

/// One stop of a gradient fill: where it sits from 0 to 1 and its color.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct GradientStop {
    pub position: f64,
    pub color: (u8, u8, u8),
}

/// A stage fill in the form the project file stores it. The panel's own model
/// is a local `FillValue`; this is the serialisable shape the background keeps
/// so reopening a recording can rebuild the painter it was saved with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum FillSpec {
    Color {
        rgb: [u8; 3],
    },
    Gradient {
        kind: GradientKind,
        stops: Vec<GradientStop>,
        angle: f64,
    },
    Image {
        path: PathBuf,
    },
}

/// The fill chosen in the Background panel. Shared by the panel, which sets
/// it, and the stage, which paints it behind the video.
#[derive(Clone)]
pub(crate) struct Background {
    selection: Rc<RefCell<Option<String>>>,
    custom_fill: Rc<RefCell<Option<FillPainter>>>,
    /// The serialisable form of `custom_fill`, kept beside the painter so the
    /// project file and the panel can read the fill the stage is painting.
    fill_spec: Rc<RefCell<Option<FillSpec>>>,
    padding: Rc<Cell<f64>>,
    radius: Rc<Cell<f64>>,
    /// The frame the project file names, or `None` for the recording's own shape.
    aspect: Rc<RefCell<Option<String>>>,
    listeners: ChangeListeners,
}

type ChangeListeners = Rc<RefCell<Vec<Rc<dyn Fn()>>>>;

impl Default for Background {
    fn default() -> Self {
        Self {
            selection: Rc::new(RefCell::new(None)),
            custom_fill: Rc::new(RefCell::new(None)),
            fill_spec: Rc::new(RefCell::new(None)),
            padding: Rc::new(Cell::new(DEFAULT_PADDING)),
            radius: Rc::new(Cell::new(DEFAULT_RADIUS)),
            aspect: Rc::new(RefCell::new(None)),
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
            custom_fill: self.custom_fill.clone(),
            fill_spec: self.fill_spec.clone(),
            padding: self.padding.clone(),
            radius: self.radius.clone(),
        }
    }

    /// The stage as a project file stores it: the wallpaper, the inset and the
    /// card's corner radius.
    pub fn snapshot(&self) -> (Option<String>, f64, f64) {
        (
            self.selection.borrow().clone(),
            self.padding.get(),
            self.radius.get(),
        )
    }

    /// Put the stage back to what a project file recorded, or to no fill at all
    /// when the recording carries no stage.
    ///
    /// The panel's fill is part of the stage now, so the painter is rebuilt
    /// from the spec the file carries rather than always dropped. Every value
    /// lands before the single notification, so a listener never reads a stage
    /// that is only half replaced, and a stage that already matches is left
    /// alone.
    pub fn apply(
        &self,
        selection: Option<&str>,
        padding: f64,
        radius: f64,
        fill: Option<&FillSpec>,
    ) {
        let selection = selection.map(str::to_owned);
        // The two fills are exclusive: a wallpaper wins over a saved painter.
        let fill = if selection.is_some() {
            None
        } else {
            fill.cloned()
        };
        let padding = stage_metric(padding, DEFAULT_PADDING);
        let radius = stage_metric(radius, DEFAULT_RADIUS);
        let unchanged = *self.selection.borrow() == selection
            && (self.padding.get() - padding).abs() <= f64::EPSILON
            && (self.radius.get() - radius).abs() <= f64::EPSILON
            && *self.fill_spec.borrow() == fill;
        if unchanged {
            return;
        }
        *self.selection.borrow_mut() = selection;
        *self.fill_spec.borrow_mut() = fill.clone();
        *self.custom_fill.borrow_mut() = fill.as_ref().and_then(painter_for);
        self.padding.set(padding);
        self.radius.set(radius);
        self.notify();
    }

    pub fn select(&self, file_name: Option<&str>) {
        let next = file_name.map(str::to_owned);
        let current = self.selection.borrow().clone();
        if current == next {
            return;
        }
        // The two fills are exclusive: a wallpaper replaces a custom one, so the
        // stage never has to decide which of them wins.
        if next.is_some() {
            *self.custom_fill.borrow_mut() = None;
            *self.fill_spec.borrow_mut() = None;
        }
        *self.selection.borrow_mut() = next;
        self.notify();
    }

    /// Install the panel's own fill, or clear it. Setting one drops the
    /// wallpaper selection for the same reason [`select`](Self::select) drops it.
    ///
    /// The spec is the model; the painter is derived from it so the stage has
    /// pixels to draw and the project file has a value to save.
    pub fn set_custom_fill(&self, spec: Option<FillSpec>) {
        let picking = spec.is_some();
        let paint = spec.as_ref().and_then(painter_for);
        *self.fill_spec.borrow_mut() = spec;
        *self.custom_fill.borrow_mut() = paint;
        if picking {
            *self.selection.borrow_mut() = None;
        }
        self.notify();
    }

    /// The panel's own fill in the form the project file stores.
    pub fn fill_spec(&self) -> Option<FillSpec> {
        self.fill_spec.borrow().clone()
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

    pub fn aspect(&self) -> Option<String> {
        self.aspect.borrow().clone()
    }

    /// Change the frame; listeners hear about it like any other stage change.
    pub fn set_aspect(&self, aspect: Option<String>) {
        if *self.aspect.borrow() == aspect {
            return;
        }
        *self.aspect.borrow_mut() = aspect;
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
pub(crate) struct BackgroundState {
    selection: Rc<RefCell<Option<String>>>,
    custom_fill: Rc<RefCell<Option<FillPainter>>>,
    fill_spec: Rc<RefCell<Option<FillSpec>>>,
    padding: Rc<Cell<f64>>,
    radius: Rc<Cell<f64>>,
}

impl BackgroundState {
    pub fn selection(&self) -> Option<String> {
        self.selection.borrow().clone()
    }

    /// True while a fill is picked, whether a wallpaper or the panel's own
    /// painter. The stage insets and grows the canvas only then, so an empty
    /// background leaves the video edge to edge.
    pub fn has_fill(&self) -> bool {
        self.selection.borrow().is_some() || self.fill_spec.borrow().is_some()
    }

    /// The panel's own fill in the form the project file stores.
    pub fn fill_spec(&self) -> Option<FillSpec> {
        self.fill_spec.borrow().clone()
    }

    /// The panel's own fill, for a stage with no wallpaper picked.
    pub fn custom_fill(&self) -> Option<FillPainter> {
        self.custom_fill.borrow().clone()
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
pub(crate) fn asset_directory() -> Option<PathBuf> {
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

/// A picture the user picked as the stage's background, decoded once and kept
/// by the painter that draws it.
pub(super) fn load_picture(path: &Path) -> Option<Pixbuf> {
    Pixbuf::from_file(path).ok()
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

/// Build the painter for a saved fill. The stage only needs pixels, so every
/// fill shape becomes one closure here instead of in the panels that choose it.
/// An image that cannot be decoded has no painter; the spec stays in the model
/// so the panel can still name the file the project asked for.
fn painter_for(spec: &FillSpec) -> Option<FillPainter> {
    match spec {
        FillSpec::Image { path } => {
            let pixbuf = load_picture(path)?;
            Some(Rc::new(move |context, width, height| {
                paint_cover(context, width, height, &pixbuf);
            }))
        }
        spec => {
            let spec = spec.clone();
            Some(Rc::new(move |context, width, height| {
                super::custom_fill::paint_spec(context, width, height, &spec);
            }))
        }
    }
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

    #[test]
    fn applying_a_project_replaces_the_whole_stage() {
        let background = Background::default();
        background.select(Some("wallpaper-004.jpg"));
        background.set_padding(48.0);
        background.set_radius(6.0);

        background.apply(Some("wallpaper-001.jpg"), 12.0, 3.0, None);
        assert_eq!(
            background.state().selection().as_deref(),
            Some("wallpaper-001.jpg")
        );
        assert_eq!(background.padding(), 12.0);
        assert_eq!(background.radius(), 3.0);
    }

    #[test]
    fn applying_no_fill_clears_a_wallpaper() {
        let background = Background::default();
        background.set_padding(48.0);
        background.select(Some("wallpaper-004.jpg"));

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS, None);

        assert!(background.state().selection().is_none());
        assert_eq!(background.padding(), DEFAULT_PADDING);
        assert_eq!(background.radius(), DEFAULT_RADIUS);
    }

    #[test]
    fn a_document_without_a_project_clears_the_previous_one() {
        let background = Background::default();
        background.select(Some("wallpaper-001.jpg"));
        background.set_padding(60.0);
        background.set_radius(12.0);
        background.set_custom_fill(Some(super::FillSpec::Color { rgb: [1, 2, 3] }));

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS, None);

        assert!(background.state().selection().is_none());
        assert!(
            background.state().custom_fill().is_none(),
            "the previous document's painter must not paint the next one"
        );
        assert_eq!(background.padding(), DEFAULT_PADDING);
        assert_eq!(background.radius(), DEFAULT_RADIUS);
    }

    #[test]
    fn a_saved_fill_rebuilds_its_painter() {
        let background = Background::default();
        let spec = super::FillSpec::Color { rgb: [12, 34, 56] };

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS, Some(&spec));

        assert_eq!(background.fill_spec(), Some(spec));
        assert!(background.state().custom_fill().is_some());
        assert!(background.state().has_fill());

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS, None);

        assert_eq!(background.fill_spec(), None);
        assert!(background.state().custom_fill().is_none());
        assert!(!background.state().has_fill());
    }

    #[test]
    fn a_wallpaper_drops_a_saved_fill() {
        let background = Background::default();
        let spec = super::FillSpec::Color { rgb: [12, 34, 56] };

        background.apply(
            Some("wallpaper-004.jpg"),
            DEFAULT_PADDING,
            DEFAULT_RADIUS,
            Some(&spec),
        );

        assert_eq!(
            background.state().selection().as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert_eq!(background.fill_spec(), None);
        assert!(background.state().custom_fill().is_none());
    }

    #[test]
    fn applying_a_stage_notifies_once_after_every_value_lands() {
        let background = Background::default();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        {
            let state = background.state();
            let seen = seen.clone();
            background.connect(std::rc::Rc::new(move || {
                seen.borrow_mut()
                    .push((state.selection(), state.padding(), state.radius()));
            }));
        }
        seen.borrow_mut().clear();

        background.apply(Some("wallpaper-004.jpg"), 48.0, 6.0, None);

        let seen = seen.borrow();
        assert_eq!(seen.len(), 1, "one redraw per applied stage");
        assert_eq!(seen[0].0.as_deref(), Some("wallpaper-004.jpg"));
        assert_eq!(seen[0].1, 48.0);
        assert_eq!(seen[0].2, 6.0);
    }

    #[test]
    fn a_stage_already_in_place_does_not_repaint() {
        let background = Background::default();
        background.select(Some("wallpaper-004.jpg"));
        background.set_padding(48.0);
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = calls.clone();
        background.connect(std::rc::Rc::new(move || counter.set(counter.get() + 1)));
        assert_eq!(calls.get(), 1, "connect paints once");

        background.apply(Some("wallpaper-004.jpg"), 48.0, DEFAULT_RADIUS, None);
        assert_eq!(calls.get(), 1, "an unchanged stage is not a redraw");
    }

    #[test]
    fn a_saved_stage_with_nonsense_metrics_reads_as_defaults() {
        let background = Background::default();

        background.apply(Some("wallpaper-004.jpg"), f64::NAN, -8.0, None);
        assert_eq!(background.padding(), DEFAULT_PADDING);
        assert_eq!(background.radius(), DEFAULT_RADIUS);

        background.apply(None, -12.0, f64::INFINITY, None);
        assert_eq!(background.padding(), 0.0);
        assert_eq!(background.radius(), DEFAULT_RADIUS);
    }
}
