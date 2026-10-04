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
use std::cell::{Cell, OnceCell, RefCell};

pub(super) fn relative_camera_transform(
    current: (f64, f64, f64, f64),
    sample: (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let sx = sample.2 / current.2;
    let sy = sample.3 / current.3;
    (sample.0 - current.0 * sx, sample.1 - current.1 * sy, sx, sy)
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SquircleClip {
        pub radius: Cell<f64>,
        pub camera_samples: RefCell<Vec<(f64, f64, f64, f64)>>,
        pub camera_layer: OnceCell<gtk4::Overlay>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SquircleClip {
        const NAME: &'static str = "ApexShotSquircleClip";
        type Type = super::SquircleClip;
        type ParentType = gtk4::Box;
    }

    impl ObjectImpl for SquircleClip {
        fn constructed(&self) {
            self.parent_constructed();
            let layer = gtk4::Overlay::new();
            layer.set_hexpand(true);
            layer.set_vexpand(true);
            gtk4::prelude::BoxExt::append(&*self.obj(), &layer);
            self.camera_layer.set(layer).unwrap();
        }
    }

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
                self.snapshot_camera(snapshot);
                snapshot.pop();
            } else {
                self.snapshot_camera(snapshot);
            }
        }
    }

    impl SquircleClip {
        fn snapshot_camera(&self, snapshot: &gtk4::Snapshot) {
            let samples = self.camera_samples.borrow();
            if samples.is_empty() {
                self.parent_snapshot(snapshot);
                return;
            }
            let content = gtk4::Snapshot::new();
            self.parent_snapshot(&content);
            let Some(node) = content.to_node() else {
                return;
            };
            let mut average = node.clone();
            for (index, &(tx, ty, sx, sy)) in samples.iter().enumerate() {
                let transform = gtk4::gsk::Transform::new()
                    .translate(&gtk4::graphene::Point::new(tx as f32, ty as f32))
                    .scale(sx as f32, sy as f32);
                let sample = gtk4::gsk::TransformNode::new(&node, &transform);
                average =
                    gtk4::gsk::CrossFadeNode::new(&average, &sample, 1.0 / (index + 2) as f32)
                        .upcast();
            }
            snapshot.append_node(&average);
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

    pub(super) fn append(&self, child: &impl IsA<gtk4::Widget>) {
        self.imp()
            .camera_layer
            .get()
            .unwrap()
            .set_child(Some(child));
    }

    pub(super) fn add_camera_overlay(&self, child: &impl IsA<gtk4::Widget>) {
        self.imp().camera_layer.get().unwrap().add_overlay(child);
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

    pub(super) fn set_camera_samples(&self, samples: Vec<(f64, f64, f64, f64)>) {
        let imp = self.imp();
        if *imp.camera_samples.borrow() == samples {
            return;
        }
        imp.camera_samples.replace(samples);
        self.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    #[test]
    fn camera_sample_snapshot_is_normalized_before_the_card_mask() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let clip = super::SquircleClip::new();
            let content = gtk4::DrawingArea::new();
            content.set_hexpand(true);
            content.set_vexpand(true);
            content.set_draw_func(|_, cr, width, height| {
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.paint().unwrap();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                for x in (0..width).step_by(16) {
                    cr.rectangle(x as f64, 0.0, 8.0, height as f64);
                }
                cr.fill().unwrap();
            });
            clip.append(&content);
            let cursor = gtk4::DrawingArea::new();
            cursor.set_hexpand(true);
            cursor.set_vexpand(true);
            cursor.set_draw_func(|_, cr, _, _| {
                cr.set_source_rgb(1.0, 0.2, 0.1);
                cr.rectangle(144.0, 96.0, 16.0, 16.0);
                cr.fill().unwrap();
            });
            clip.add_camera_overlay(&cursor);
            let window = gtk4::Window::new();
            window.set_default_size(320, 240);
            window.set_child(Some(&clip));
            window.present();
            for _ in 0..10 {
                while gtk4::glib::MainContext::default().pending() {
                    gtk4::glib::MainContext::default().iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            assert!(clip.width() >= 320);
            let viewport = gtk4::graphene::Rect::new(0.0, 0.0, 320.0, 240.0);
            let render = || {
                let renderer = gtk4::gsk::CairoRenderer::new();
                renderer.realize(None::<&gtk4::gdk::Surface>).unwrap();
                let snapshot = gtk4::Snapshot::new();
                clip.imp().snapshot(&snapshot);
                let node = snapshot.to_node().unwrap();
                let texture = renderer.render_texture(&node, Some(&viewport));
                renderer.unrealize();
                let mut pixels = vec![0; 320 * 240 * 4];
                texture.download(&mut pixels, 320 * 4);
                (texture, pixels)
            };
            let (sharp_texture, sharp) = render();
            let cursor_pixel = &sharp[(100 * 320 + 148) * 4..(100 * 320 + 148) * 4 + 3];
            assert!(cursor_pixel.iter().max() > cursor_pixel.iter().min());
            clip.set_camera_samples(vec![(0.0, 0.0, 1.0, 1.0); 7]);
            let (_, stationary) = render();
            assert_eq!(sharp, stationary);
            clip.set_radius(24.0);
            clip.set_camera_samples((1..8).map(|index| (index as f64, 0.0, 1.0, 1.0)).collect());
            let (texture, blurred) = render();
            let pixel = |x: usize, y: usize| (y * 320 + x) * 4;
            assert_eq!(blurred[pixel(0, 0) + 3], 0);
            assert_eq!(blurred[pixel(100, 100) + 3], 255);
            assert!(blurred[pixel(21, 100)] > 0 && blurred[pixel(21, 100)] < 255);
            let sharp_sum: u64 = (32..288).map(|x| sharp[pixel(x, 120)] as u64).sum();
            let blur_sum: u64 = (32..288).map(|x| blurred[pixel(x, 120)] as u64).sum();
            assert!(
                sharp_sum.abs_diff(blur_sum) <= 256,
                "averaging must not darken footage"
            );
            let dir = std::path::Path::new("target/test-fixtures");
            std::fs::create_dir_all(dir).unwrap();
            sharp_texture
                .save_to_png(dir.join("camera-blur-preview-sharp.png"))
                .unwrap();
            texture
                .save_to_png(dir.join("camera-blur-preview.png"))
                .unwrap();
            window.close();
        }) else {
            return;
        };
    }

    #[test]
    fn relative_camera_samples_match_the_export_view_mapping() {
        use crate::recording::editor::model::{source_to_zoomed_point, zoom_camera_transform};
        let current_view = (140.0, 80.0, 960.0, 540.0);
        let earlier_view = (130.0, 75.0, 980.0, 552.0);
        let current = zoom_camera_transform(current_view, 1920.0, 1080.0, 640.0, 360.0);
        let earlier = zoom_camera_transform(earlier_view, 1920.0, 1080.0, 640.0, 360.0);
        let (tx, ty, sx, sy) = super::relative_camera_transform(current, earlier);
        let point = source_to_zoomed_point(600.0, 300.0, current_view, 640.0, 360.0);
        let expected = source_to_zoomed_point(600.0, 300.0, earlier_view, 640.0, 360.0);
        assert!((point.0 * sx + tx - expected.0).abs() < 1e-9);
        assert!((point.1 * sy + ty - expected.1).abs() < 1e-9);
        assert_eq!(
            super::relative_camera_transform(current, current),
            (0.0, 0.0, 1.0, 1.0)
        );
    }

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
