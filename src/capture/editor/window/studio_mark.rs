//! The Studio preview mark on the live canvas.
//!
//! An unlicensed document that uses a Studio style gets the "Made with
//! ApexShot" mark painted over the canvas, so the preview cannot be captured
//! clean from the screen. Only the screen carries it: saves, copies and
//! uploads go through the Studio card, which stops them before any pixels are
//! written.

use gtk4::cairo::{Context, Operator};
use gtk4::{glib, prelude::*, DrawingArea};
use image::RgbaImage;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::capture::editor::{state::EditorState, studio_watermark as mark};
use mark::ScreenRect;

/// How long a licence read stays good for painting.
const UNLOCK_CACHE_TTL: Duration = Duration::from_secs(1);

/// How often the canvas checks whether the mark should appear or go away.
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Per-window state for the mark: the cached licence read and the label's
/// measured width. Clones share the same cells.
#[derive(Clone, Default)]
pub(super) struct StudioMark {
    unlocked: Rc<Cell<Option<(Instant, bool)>>>,
    text_ratio: Rc<Cell<Option<f64>>>,
}

impl StudioMark {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// The licence state, re-read at most once per `UNLOCK_CACHE_TTL` so
    /// painting never touches the entitlement file every frame.
    fn unlocked(&self) -> bool {
        let now = Instant::now();
        if let Some((at, value)) = self.unlocked.get() {
            if now.duration_since(at) < UNLOCK_CACHE_TTL {
                return value;
            }
        }
        let value = crate::studio_entitlement::studio_unlocked();
        self.unlocked.set(Some((now, value)));
        value
    }

    /// Forget the cached licence state, e.g. right after activation.
    pub(super) fn invalidate(&self) {
        self.unlocked.set(None);
    }

    /// Paint the mark over the visible part of the canvas. Every rectangle is
    /// in widget pixels: `canvas` is the whole composition (wallpaper
    /// included), `image` the screenshot card and `visible` the drawing area.
    pub(super) fn paint(
        &self,
        context: &Context,
        uses_style: bool,
        working_image: &RgbaImage,
        canvas: ScreenRect,
        image: ScreenRect,
        visible: ScreenRect,
    ) {
        if !mark_due(uses_style, || self.unlocked()) {
            return;
        }
        let Some(region) = canvas.intersect(visible) else {
            return;
        };
        let ratio = self.text_ratio.get().unwrap_or_else(|| {
            let ratio = mark::text_ratio(context);
            self.text_ratio.set(Some(ratio));
            ratio
        });
        let stage = mark::Stage {
            canvas,
            image,
            visible,
        };
        let Some(rect) = mark::place(stage, ratio) else {
            return;
        };
        let ink = mark::ink_for_image(working_image, rect, image);
        let _ = context.save();
        context.set_operator(Operator::Over);
        context.rectangle(region.x, region.y, region.width, region.height);
        context.clip();
        mark::paint(context, rect, ink);
        let _ = context.restore();
    }
}

/// Whether the mark is due: the document uses a Studio style and no licence
/// covers this build. The licence is only read when a style needs it.
fn mark_due(uses_style: bool, unlocked: impl FnOnce() -> bool) -> bool {
    uses_style && !unlocked()
}

/// Repaint the canvas as the mark appears or goes away: a Studio style is
/// switched on or off, or a licence is activated elsewhere in the app. Each
/// tick reads the document under its lock once and the licence from cache.
pub(super) fn install_refresh(
    drawing_area: &DrawingArea,
    state: Arc<Mutex<EditorState>>,
    studio_mark: StudioMark,
) {
    let weak = drawing_area.downgrade();
    let last = Cell::new(false);
    let recorded = Cell::new(false);
    glib::timeout_add_local(REFRESH_INTERVAL, move || {
        let Some(area) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let uses_style = state
            .lock()
            .map(|st| st.uses_studio_style())
            .unwrap_or(false);
        let due = mark_due(uses_style, || studio_mark.unlocked());
        if due && !recorded.replace(true) {
            crate::usage_telemetry::record_studio_tool_opened();
        }
        if due != last.replace(due) {
            area.queue_draw();
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::cairo::{Context, Format, ImageSurface};

    #[test]
    fn the_mark_is_due_only_for_a_studio_style_without_a_licence() {
        assert!(mark_due(true, || false));
        assert!(!mark_due(true, || true));
        assert!(!mark_due(false, || false));
        // No style needs the mark, so the licence is never read.
        assert!(!mark_due(false, || panic!("the licence must not be read")));
    }

    /// Paint over a 400x300 canvas whose left 200 columns are visible, then
    /// report the columns that carry bright mark ink.
    fn bright_columns(uses_style: bool, licensed: bool) -> Vec<usize> {
        let mark = StudioMark::new();
        // Seed the licence cache so the test never reads the entitlement file.
        mark.unlocked.set(Some((Instant::now(), licensed)));
        let rect = |x, y, width, height| ScreenRect {
            x,
            y,
            width,
            height,
        };
        let canvas = rect(0.0, 0.0, 400.0, 300.0);
        let card = rect(100.0, 100.0, 200.0, 100.0);
        let visible = rect(0.0, 0.0, 200.0, 300.0);
        let image = RgbaImage::from_pixel(200, 100, image::Rgba([20, 20, 30, 255]));
        let mut surface = ImageSurface::create(Format::ARgb32, 400, 300).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().unwrap();
            mark.paint(&context, uses_style, &image, canvas, card, visible);
        }
        surface.flush();
        let stride = usize::try_from(surface.stride()).unwrap();
        let data = surface.data().unwrap();
        let mut columns: Vec<usize> = (0..300)
            .flat_map(|y| (0..400).map(move |x| (x, y)))
            .filter(|&(x, y)| data[y * stride + x * 4 + 1] > 128)
            .map(|(x, _)| x)
            .collect();
        columns.sort_unstable();
        columns.dedup();
        columns
    }

    #[test]
    fn the_mark_paints_only_over_a_visible_studio_style_without_a_licence() {
        let painted = bright_columns(true, false);
        assert!(!painted.is_empty(), "the mark is painted");
        assert!(
            painted.iter().all(|&x| x < 200),
            "the mark stays on the visible canvas: {painted:?}"
        );
        assert!(bright_columns(false, false).is_empty());
        assert!(bright_columns(true, true).is_empty());
    }
}
