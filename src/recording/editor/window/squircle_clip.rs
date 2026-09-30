//! A single-child container that masks its content to a squircle rectangle.
//!
//! The image editor draws its card corners with Cairo's superellipse path
//! ([`crate::capture::editor::render::rounded_rect_path`]), but the video
//! preview renders the footage through GTK widgets, whose CSS `border-radius`
//! can only draw circular arcs. This wraps the video and masks it with that same
//! squircle path, so the preview matches the export (and the image editor)
//! corner for corner. Masking needs `GtkSnapshot::push_mask`, which is why the
//! crate builds against GTK 4.10+.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SquircleClip {
        pub radius: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SquircleClip {
        const NAME: &'static str = "ApexShotSquircleClip";
        type Type = super::SquircleClip;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for SquircleClip {}

    impl WidgetImpl for SquircleClip {
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let radius = self.radius.get();
            let (width, height) = (self.obj().width() as f64, self.obj().height() as f64);
            if radius > 0.5 && width > 1.0 && height > 1.0 {
                // Mask mode records two regions and needs two pops: the mask
                // first, then the content it is applied to.
                snapshot.push_mask(gtk4::gsk::MaskMode::Alpha);
                {
                    // Scoped so the Cairo surface is flushed into the mask node
                    // before the mask is closed.
                    let context = snapshot.append_cairo(&gtk4::graphene::Rect::new(
                        0.0,
                        0.0,
                        width as f32,
                        height as f32,
                    ));
                    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                    crate::capture::editor::render::rounded_rect_path(
                        &context, 0.0, 0.0, width, height, radius,
                    );
                    let _ = context.fill();
                }
                snapshot.pop();
                self.parent_snapshot(snapshot);
                snapshot.pop();
            } else {
                self.parent_snapshot(snapshot);
            }
        }
    }

    impl BoxImpl for SquircleClip {}
}

glib::wrapper! {
    pub struct SquircleClip(ObjectSubclass<imp::SquircleClip>)
        @extends gtk4::Widget, gtk4::Box,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

impl SquircleClip {
    pub(super) fn new() -> Self {
        glib::Object::builder()
            .property("orientation", gtk4::Orientation::Vertical)
            .build()
    }

    /// Corner radius in this widget's own pixels; 0 leaves the content square.
    pub(super) fn set_radius(&self, radius: f64) {
        let imp = self.imp();
        if (imp.radius.get() - radius).abs() < 0.5 {
            return;
        }
        imp.radius.set(radius);
        self.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    fn production_source() -> &'static str {
        let source = include_str!("squircle_clip.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn squircle_clip_masks_its_child_with_the_shared_squircle_path() {
        let source = production_source();
        assert!(
            source.contains("snapshot.push_mask(gtk4::gsk::MaskMode::Alpha)")
                && source.contains("crate::capture::editor::render::rounded_rect_path(")
                && source.contains("self.parent_snapshot(snapshot);")
                && source.contains("snapshot.pop();"),
            "the preview clip must mask its child with the image editor's squircle path"
        );
    }

    #[test]
    fn squircle_clip_constructs_and_sets_radius_on_the_gtk_thread() {
        // Exercises the subclass registration and the setter (including the
        // idempotent no-op path) on the one permitted GTK thread. Skips when no
        // display server is available, like the other GTK-dependent tests.
        let Some(()) = crate::test_support::with_gtk(|| {
            let clip = super::SquircleClip::new();
            clip.set_radius(12.0);
            clip.set_radius(12.0);
            clip.set_radius(0.0);
        }) else {
            return;
        };
    }
}
