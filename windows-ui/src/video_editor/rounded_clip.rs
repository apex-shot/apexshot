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
            let radius = scaled_radius(self.radius.get(), self.canvas_width.get(), width)
                .min(width.min(height) / 2.0);
            if radius <= 0.5 || width <= 1.0 || height <= 1.0 {
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

    /// Set the card's corner radius in export-canvas pixels together with the
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
    use super::scaled_radius;

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
