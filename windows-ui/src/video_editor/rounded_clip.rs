//! Rounded clipping for the preview's video card.
//!
//! The card is clipped by a native rounded clip node rather than by fading
//! frame pixels or painting a mask. The renderer antialiases the clip at the
//! monitor's scale, so a large radius stays smooth instead of stepping, and
//! clipping costs nothing extra per frame.

use gtk4::glib;
use gtk4::graphene;
use gtk4::gsk;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::Box as GtkBox;
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RoundedClip {
        /// Corner radius in export-canvas pixels, or zero to skip the clip.
        pub radius: Cell<f64>,
        /// Width the radius was measured against; the widget rescales it to its
        /// own width when it paints.
        pub canvas_width: Cell<f64>,
        /// The pane the child is clipped to, as fractions of the widget: the
        /// split screen's share of the frame.
        pub layout_rect: Cell<Option<(f64, f64, f64, f64)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoundedClip {
        const NAME: &'static str = "ApexShotWindowsRoundedClip";
        type Type = super::RoundedClip;
        type ParentType = GtkBox;
    }

    impl ObjectImpl for RoundedClip {}

    impl gtk4::subclass::box_::BoxImpl for RoundedClip {}

    impl WidgetImpl for RoundedClip {
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let widget = self.obj();
            let width = f64::from(widget.width());
            let height = f64::from(widget.height());
            if width <= 1.0 || height <= 1.0 {
                self.parent_snapshot(snapshot);
                return;
            }
            if let Some(rect) = self.layout_rect.get() {
                let (x, y, w, h) = rect;
                if w > 0.0
                    && h > 0.0
                    && !(x.abs() <= 1e-9
                        && y.abs() <= 1e-9
                        && (w - 1.0).abs() <= 1e-9
                        && (h - 1.0).abs() <= 1e-9)
                {
                    self.snapshot_layout(snapshot, width, height, rect);
                    return;
                }
            }
            let radius = scaled_radius(self.radius.get(), self.canvas_width.get(), width)
                .min(width.min(height) / 2.0);
            if radius <= 0.5 {
                self.parent_snapshot(snapshot);
                return;
            }
            let corner = graphene::Size::new(radius as f32, radius as f32);
            let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
            let clip = gsk::RoundedRect::new(bounds, corner, corner, corner, corner);
            snapshot.push_rounded_clip(&clip);
            self.parent_snapshot(snapshot);
            snapshot.pop();
        }
    }

    impl RoundedClip {
        /// Draw the child into a normalized layout pane, clipped to the same
        /// card shape. The whole source scales uniformly until it covers the
        /// pane, so the split crops the footage instead of stretching it, and
        /// the corner radius is measured against the pane's width so the card's
        /// corner keeps its pixel size.
        fn snapshot_layout(
            &self,
            snapshot: &gtk4::Snapshot,
            width: f64,
            height: f64,
            rect: (f64, f64, f64, f64),
        ) {
            let placement = super::pane_placement(width, height, rect);
            let (clip_x, clip_y, clip_width, clip_height) = placement.clip;
            let radius = scaled_radius(self.radius.get(), self.canvas_width.get(), clip_width)
                .min(clip_width.min(clip_height) / 2.0);
            let bounds = graphene::Rect::new(
                clip_x as f32,
                clip_y as f32,
                clip_width as f32,
                clip_height as f32,
            );
            if radius > 0.5 {
                let corner = graphene::Size::new(radius as f32, radius as f32);
                let clip = gsk::RoundedRect::new(bounds, corner, corner, corner, corner);
                snapshot.push_rounded_clip(&clip);
            } else {
                snapshot.push_clip(&bounds);
            }
            let (draw_x, draw_y, draw_width, draw_height) = placement.draw;
            snapshot.save();
            snapshot.translate(&graphene::Point::new(draw_x as f32, draw_y as f32));
            snapshot.scale((draw_width / width) as f32, (draw_height / height) as f32);
            self.parent_snapshot(snapshot);
            snapshot.restore();
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    pub struct RoundedClip(ObjectSubclass<imp::RoundedClip>)
        @extends GtkBox, gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl RoundedClip {
    pub(super) fn new(child: &impl IsA<gtk4::Widget>) -> Self {
        let clip: Self = glib::Object::new();
        clip.append(child);
        clip
    }

    /// Set the corner radius in export-canvas pixels together with the
    /// width it was measured against (the video rect's width). A zero radius
    /// paints the child unclipped.
    pub(super) fn set_radius(&self, radius: f64, canvas_width: f64) {
        let radius = if radius.is_finite() && radius > 0.0 {
            radius
        } else {
            0.0
        };
        let canvas_width = if canvas_width.is_finite() {
            canvas_width.max(1.0)
        } else {
            1.0
        };
        if (self.imp().radius.get() - radius).abs() < 1e-9
            && (self.imp().canvas_width.get() - canvas_width).abs() < 1e-9
        {
            return;
        }
        self.imp().radius.set(radius);
        self.imp().canvas_width.set(canvas_width);
        self.queue_draw();
    }

    /// Clip the child to a normalized pane of the card. `None` fills the card,
    /// which is every layout but the split's screen pane.
    pub(super) fn set_layout_rect(&self, rect: Option<(f64, f64, f64, f64)>) {
        let rect = rect
            .map(|(x, y, w, h)| {
                (
                    x.clamp(0.0, 1.0),
                    y.clamp(0.0, 1.0),
                    w.clamp(0.0, 1.0),
                    h.clamp(0.0, 1.0),
                )
            })
            .filter(|(x, y, w, h)| {
                *w > 1e-6
                    && *h > 1e-6
                    && !(x.abs() <= 1e-9
                        && y.abs() <= 1e-9
                        && (w - 1.0).abs() <= 1e-9
                        && (h - 1.0).abs() <= 1e-9)
            });
        if self.imp().layout_rect.get() == rect {
            return;
        }
        self.imp().layout_rect.set(rect);
        self.queue_draw();
    }
}

/// Where a normalized layout pane lands in a `width` × `height` box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PanePlacement {
    /// The pane itself, which bounds the rounded clip.
    pub clip: (f64, f64, f64, f64),
    /// The whole source scaled uniformly until it covers the pane and centred
    /// on it, which the pane then crops.
    pub draw: (f64, f64, f64, f64),
}

/// Place a normalized layout pane, and the whole source, in a box.
pub(super) fn pane_placement(
    width: f64,
    height: f64,
    (x, y, w, h): (f64, f64, f64, f64),
) -> PanePlacement {
    let cover = super::webcam::screen_cover_rect(super::webcam::Rect {
        x,
        y,
        width: w,
        height: h,
    });
    PanePlacement {
        clip: (x * width, y * height, w * width, h * height),
        draw: (
            cover.x * width,
            cover.y * height,
            cover.width * width,
            cover.height * height,
        ),
    }
}

/// Rescale a canvas-space radius to the widget's own width, the way the export
/// rescales it to the video rect, so the preview corner reads the same at any
/// preview size.
fn scaled_radius(radius: f64, canvas_width: f64, width: f64) -> f64 {
    if radius <= 0.0 || canvas_width <= 0.0 || width <= 0.0 {
        return 0.0;
    }
    radius * width / canvas_width
}

#[cfg(test)]
mod tests {
    use super::{pane_placement, scaled_radius};

    #[test]
    fn a_landscape_split_covers_its_pane() {
        let placement = pane_placement(320.0, 240.0, (0.0, 0.0, 0.5, 1.0));
        assert_eq!(placement.clip, (0.0, 0.0, 160.0, 240.0));
        assert_eq!(placement.draw, (-80.0, 0.0, 320.0, 240.0));
        assert_eq!(placement.draw.3, placement.clip.3);
        assert!((placement.draw.2 / placement.draw.3 - 320.0 / 240.0).abs() < 1e-9);
    }

    #[test]
    fn a_portrait_split_covers_its_pane() {
        let placement = pane_placement(240.0, 320.0, (0.0, 0.5, 1.0, 0.5));
        assert_eq!(placement.clip, (0.0, 160.0, 240.0, 160.0));
        assert_eq!(placement.draw, (0.0, 80.0, 240.0, 320.0));
        assert_eq!(placement.draw.2, placement.clip.2);
        assert!((placement.draw.3 / placement.draw.2 - 320.0 / 240.0).abs() < 1e-9);
    }

    #[test]
    fn a_flipped_split_centres_the_cover_on_the_other_pane() {
        let placement = pane_placement(320.0, 240.0, (0.5, 0.0, 0.5, 1.0));
        assert_eq!(placement.clip, (160.0, 0.0, 160.0, 240.0));
        assert_eq!(placement.draw, (80.0, 0.0, 320.0, 240.0));
    }

    #[test]
    fn a_proportional_pane_is_its_own_cover() {
        let placement = pane_placement(320.0, 240.0, (0.25, 0.25, 0.5, 0.5));
        assert_eq!(placement.clip, (80.0, 60.0, 160.0, 120.0));
        assert_eq!(placement.draw, (80.0, 60.0, 160.0, 120.0));
    }

    #[test]
    fn a_full_frame_stays_unscaled() {
        let placement = pane_placement(320.0, 240.0, (0.0, 0.0, 1.0, 1.0));
        assert_eq!(placement.clip, (0.0, 0.0, 320.0, 240.0));
        assert_eq!(placement.draw, (0.0, 0.0, 320.0, 240.0));
    }

    #[test]
    fn the_radius_follows_the_preview_width() {
        // 40 slider units on a 1080p canvas is 192 canvas pixels; a 960px
        // preview card shows half of that.
        assert!((scaled_radius(192.0, 1920.0, 960.0) - 96.0).abs() < 1e-9);
        assert!((scaled_radius(192.0, 1920.0, 1920.0) - 192.0).abs() < 1e-9);
    }

    #[test]
    fn a_square_card_stays_square() {
        assert_eq!(scaled_radius(0.0, 1920.0, 900.0), 0.0);
        assert_eq!(scaled_radius(192.0, 1920.0, 0.0), 0.0);
        assert_eq!(scaled_radius(192.0, 0.0, 900.0), 0.0);
    }
}
