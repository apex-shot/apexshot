//! Blur-to-sharp text entrances without changing the label's shaping or layout.

use std::cell::Cell;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib, graphene, pango, Box as GtkBox, Label, Snapshot, Widget};

use super::motion::{ease_out_cubic, enabled};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BlurText {
        pub active: Cell<bool>,
        pub elapsed_ms: Cell<f64>,
        pub delay_ms: Cell<f64>,
        pub duration_ms: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BlurText {
        const NAME: &'static str = "ApexShotWindowsOnboardingBlurText";
        type Type = super::BlurText;
        type ParentType = GtkBox;
    }

    impl ObjectImpl for BlurText {}
    impl BoxImpl for BlurText {}

    impl WidgetImpl for BlurText {
        fn snapshot(&self, snapshot: &Snapshot) {
            if !self.active.get() {
                self.parent_snapshot(snapshot);
                return;
            }
            if self.elapsed_ms.get() <= self.delay_ms.get() {
                return;
            }

            let widget = self.obj();
            let Some(label) = widget.first_child().and_downcast::<Label>() else {
                return;
            };
            let clusters = text_clusters(&label);
            let source = Snapshot::new();
            self.parent_snapshot(&source);
            let Some(node) = source.to_node() else {
                return;
            };

            for (index, cluster) in clusters.iter().enumerate() {
                let progress = character_progress(
                    self.elapsed_ms.get(),
                    self.delay_ms.get(),
                    self.duration_ms.get(),
                    index,
                    clusters.len(),
                );
                if progress <= 0.0 {
                    continue;
                }
                let remaining = 1.0 - ease_out_cubic(progress);
                let blur = f64::from(cluster.bounds.height()) * 0.24 * remaining;
                let rise = cluster.bounds.height() * 0.12 * remaining as f32;
                snapshot.save();
                snapshot.translate(&graphene::Point::new(0.0, rise));
                snapshot.push_opacity(progress * progress * (3.0 - 2.0 * progress));
                snapshot.push_blur(blur);
                // Isolate the shaped cluster BEFORE blurring, so the soft edge
                // can spread outside its original bounds instead of being cut off.
                snapshot.push_clip(&cluster.bounds);
                snapshot.append_node(&node);
                snapshot.pop();
                snapshot.pop();
                snapshot.pop();
                snapshot.restore();
            }
        }
    }
}

glib::wrapper! {
    pub struct BlurText(ObjectSubclass<imp::BlurText>)
        @extends GtkBox, Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget, gtk4::Orientable;
}

/// Wrap an unparented label in a staggered blur reveal. The original label
/// remains the layout and accessibility source, including wrapping and kerning.
pub(super) fn reveal(
    label: &Label,
    delay_ms: f64,
    duration_ms: f64,
) -> (Widget, Option<gtk4::TickCallbackId>) {
    if !enabled() || label.text().is_empty() {
        return (label.clone().upcast(), None);
    }
    let widget: BlurText = glib::Object::new();
    widget.set_orientation(gtk4::Orientation::Vertical);
    widget.set_overflow(gtk4::Overflow::Visible);
    widget.append(label);
    widget.imp().active.set(true);
    widget.imp().delay_ms.set(delay_ms);
    widget.imp().duration_ms.set(duration_ms);

    let started = Cell::new(None::<i64>);
    let callback = widget.add_tick_callback(move |widget, clock| {
        let now = clock.frame_time();
        let start = started.get().unwrap_or(now);
        started.set(Some(start));
        let elapsed_ms = (now - start) as f64 / 1000.0;
        let imp = widget.imp();
        imp.elapsed_ms.set(elapsed_ms);
        let done = !enabled() || !widget.is_mapped() || elapsed_ms >= delay_ms + duration_ms;
        imp.active.set(!done);
        widget.queue_draw();
        if done {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    (widget.upcast(), Some(callback))
}

/// Keep the reveal clock linear; easing applies within each character's turn,
/// never to the stagger itself (which would skip or reverse the entrance).
fn character_progress(
    elapsed_ms: f64,
    delay_ms: f64,
    duration_ms: f64,
    index: usize,
    count: usize,
) -> f64 {
    if duration_ms <= 0.0 {
        return f64::from(elapsed_ms >= delay_ms);
    }
    let local_duration = if count <= 1 {
        duration_ms
    } else {
        duration_ms * 0.4
    };
    let stagger = (duration_ms - local_duration) / count.saturating_sub(1).max(1) as f64;
    ((elapsed_ms - delay_ms - index as f64 * stagger) / local_duration).clamp(0.0, 1.0)
}

struct TextCluster {
    index: i32,
    bounds: graphene::Rect,
}

fn text_clusters(label: &Label) -> Vec<TextCluster> {
    let layout = label.layout();
    let (offset_x, offset_y) = label.layout_offsets();
    let allocation = label.allocation();
    let scale = pango::SCALE as f32;
    let mut iter = layout.iter();
    let mut clusters = Vec::new();
    loop {
        let (ink, logical) = iter.cluster_extents();
        if ink.width() > 0 && ink.height() > 0 && logical.width() > 0 {
            clusters.push(TextCluster {
                index: iter.index(),
                bounds: graphene::Rect::new(
                    (allocation.x() + offset_x) as f32 + logical.x() as f32 / scale,
                    (allocation.y() + offset_y) as f32 + logical.y() as f32 / scale,
                    logical.width() as f32 / scale,
                    logical.height() as f32 / scale,
                ),
            });
        }
        if !iter.next_cluster() {
            break;
        }
    }
    // Pango iterates visually. Logical byte order also gives translated RTL
    // text the correct reading-order stagger, without splitting combining marks.
    clusters.sort_by_key(|cluster| cluster.index);
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_waits_for_its_delay_then_advances_from_the_first_character() {
        assert_eq!(character_progress(499.0, 500.0, 1000.0, 0, 20), 0.0);
        assert_eq!(character_progress(500.0, 500.0, 1000.0, 0, 20), 0.0);
        let first = character_progress(520.0, 500.0, 1000.0, 0, 20);
        assert!(first > 0.0 && first < 0.1);
        assert_eq!(character_progress(520.0, 500.0, 1000.0, 1, 20), 0.0);
        assert_eq!(character_progress(520.0, 500.0, 1000.0, 19, 20), 0.0);
    }

    #[test]
    fn every_character_moves_forward_and_finishes_on_schedule() {
        for index in 0..24 {
            let mut previous = 0.0;
            for elapsed in (0..=1500).step_by(10) {
                let progress = character_progress(elapsed as f64, 500.0, 1000.0, index, 24);
                assert!(progress >= previous);
                previous = progress;
            }
            assert!((previous - 1.0).abs() < 1e-9);
        }
        assert_eq!(character_progress(1000.0, 500.0, 1000.0, 0, 24), 1.0);
        assert_eq!(character_progress(1000.0, 500.0, 1000.0, 23, 24), 0.0);
    }

    #[test]
    fn single_character_and_instant_reveals_have_no_invalid_stagger() {
        assert_eq!(character_progress(1000.0, 500.0, 1000.0, 0, 1), 0.5);
        assert_eq!(character_progress(499.0, 500.0, 0.0, 0, 0), 0.0);
        assert_eq!(character_progress(500.0, 500.0, 0.0, 0, 0), 1.0);
    }

    #[test]
    fn shaped_clusters_keep_combining_marks_with_their_letter() {
        let _ = crate::test_support::with_gtk(|| {
            let label = Label::new(Some("e\u{301} x"));
            let clusters = text_clusters(&label);
            assert_eq!(
                clusters
                    .iter()
                    .map(|cluster| cluster.index)
                    .collect::<Vec<_>>(),
                vec![0, 4]
            );
        });
    }

    #[test]
    fn reduced_motion_keeps_the_original_label_visible() {
        let _ = crate::test_support::with_gtk(|| {
            let settings = gtk4::Settings::default().expect("GTK settings");
            let previous = settings.is_gtk_enable_animations();
            settings.set_gtk_enable_animations(false);
            let label = Label::new(Some("Welcome"));
            let (widget, callback) = reveal(&label, 500.0, 1000.0);
            settings.set_gtk_enable_animations(previous);
            assert_eq!(widget, label.upcast::<Widget>());
            assert!(callback.is_none());
            assert_eq!(widget.opacity(), 1.0);
        });
    }
}
