//! The pointer path the follow camera reads.
//!
//! The camera does not chase the raw recorded pointer. Before any zoom takes
//! its range from the path, the whole recording is thinned once: the shared
//! shake filter runs at the project's threshold, then moves closer than
//! [`DECIMATE_DISTANCE`] to the last kept event are dropped. Clicks are part of
//! the path and always survive the decimation, so a zoom can frame a click
//! even when the pointer did not move.
//!
//! Filtering the whole path once, rather than once per zoom window, keeps a
//! teleport the filter dropped from pulling the camera through a zoom that
//! happens to contain it. The result is cached beside the camera plan and
//! rebuilt only when the sidecar or the shake threshold changes.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::recording::editor::auto_zoom::release_position;
use crate::recording::editor::cursor_shake::{self, ShakeEvent, ShakeEventKind};
use crate::recording::editor::sidecar::PointerSidecar;

/// The studied minimum distance between kept pointer moves, in pixels.
pub const DECIMATE_DISTANCE: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomPathKind {
    Move,
    Down,
    Up,
}

/// One instant of the camera's pointer path, in encoded-video pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomPathEvent {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub kind: ZoomPathKind,
}

/// The recording's pointer path: every recorded move, every click as a press,
/// and every press release, in time order and in encoded-video pixels.
pub fn events(sidecar: &PointerSidecar, width: f64, height: f64) -> Vec<ZoomPathEvent> {
    let map = |x: f64, y: f64| sidecar.map_to_video(x, y, width, height);
    let mut events: Vec<ZoomPathEvent> = Vec::new();
    for sample in &sidecar.pointer {
        let (x, y) = map(sample.x, sample.y);
        events.push(ZoomPathEvent {
            t: sample.t,
            x,
            y,
            kind: ZoomPathKind::Move,
        });
    }
    for click in &sidecar.clicks {
        let (x, y) = map(click.x, click.y);
        events.push(ZoomPathEvent {
            t: click.t,
            x,
            y,
            kind: ZoomPathKind::Down,
        });
    }
    for press in &sidecar.presses {
        if !press.up.is_finite() || press.up < press.down {
            continue;
        }
        if let Some((x, y)) = release_position(sidecar, press, width, height) {
            events.push(ZoomPathEvent {
                t: press.up,
                x,
                y,
                kind: ZoomPathKind::Up,
            });
        }
    }
    events.retain(|event| event.t.is_finite() && event.x.is_finite() && event.y.is_finite());
    // A stable sort keeps a move ahead of a press at the same instant.
    events.sort_by(|left, right| left.t.total_cmp(&right.t));
    events
}

/// The camera's whole path for a recording: the shared shake filter at
/// `shake_threshold`, then the 10 px decimation. Zero or a non-finite
/// threshold turns the shake filter off, exactly as it does for the cursor.
pub fn filter(events: &[ZoomPathEvent], shake_threshold: f64) -> Vec<ZoomPathEvent> {
    let shake: Vec<ShakeEvent> = events
        .iter()
        .map(|event| ShakeEvent {
            time_ms: event.t * 1000.0,
            x: event.x,
            y: event.y,
            kind: match event.kind {
                ZoomPathKind::Move => ShakeEventKind::Move,
                ZoomPathKind::Down => ShakeEventKind::Down,
                ZoomPathKind::Up => ShakeEventKind::Up,
            },
        })
        .collect();
    let keep = cursor_shake::shake_keep_mask(&shake, shake_threshold);
    let kept: Vec<ZoomPathEvent> = events
        .iter()
        .zip(keep)
        .filter_map(|(event, keep)| keep.then_some(*event))
        .collect();
    decimate(&kept, DECIMATE_DISTANCE)
}

/// Presses and releases are always kept; a move is kept once it is
/// `min_distance` from the last kept event.
pub fn decimate(events: &[ZoomPathEvent], min_distance: f64) -> Vec<ZoomPathEvent> {
    let mut kept: Vec<ZoomPathEvent> = Vec::with_capacity(events.len());
    let Some((first, rest)) = events.split_first() else {
        return kept;
    };
    kept.push(*first);
    let mut last = *first;
    for event in rest {
        let keep = match event.kind {
            ZoomPathKind::Down | ZoomPathKind::Up => true,
            ZoomPathKind::Move => (event.x - last.x).hypot(event.y - last.y) >= min_distance,
        };
        if keep {
            kept.push(*event);
            last = *event;
        }
    }
    kept
}

/// The filtered path, rebuilt only when something it depends on changes.
#[derive(Debug, Default)]
pub struct ZoomPathCache {
    key: Option<u64>,
    path: Arc<Vec<ZoomPathEvent>>,
}

impl ZoomPathCache {
    /// The filtered path for this sidecar, frame and threshold. The first call
    /// for a given input builds it; later calls share the same allocation.
    pub fn get_or_build(
        &mut self,
        sidecar: Option<&PointerSidecar>,
        width: f64,
        height: f64,
        shake_threshold: f64,
    ) -> Arc<Vec<ZoomPathEvent>> {
        let key = signature(sidecar, width, height, shake_threshold);
        if self.key != Some(key) {
            self.path = Arc::new(
                sidecar
                    .map(|sidecar| filter(&events(sidecar, width, height), shake_threshold))
                    .unwrap_or_default(),
            );
            self.key = Some(key);
        }
        Arc::clone(&self.path)
    }
}

/// Everything the filtered path depends on: the samples it is built from, the
/// capture geometry that maps them into video pixels, and the threshold.
fn signature(
    sidecar: Option<&PointerSidecar>,
    width: f64,
    height: f64,
    shake_threshold: f64,
) -> u64 {
    let mut hash = DefaultHasher::new();
    width.to_bits().hash(&mut hash);
    height.to_bits().hash(&mut hash);
    shake_threshold.to_bits().hash(&mut hash);
    let Some(sidecar) = sidecar else {
        return hash.finish();
    };
    for value in [
        sidecar.region.x as f64,
        sidecar.region.y as f64,
        sidecar.region.w as f64,
        sidecar.region.h as f64,
    ] {
        value.to_bits().hash(&mut hash);
    }
    sidecar.pointer.len().hash(&mut hash);
    for sample in &sidecar.pointer {
        for value in [sample.t, sample.x, sample.y] {
            value.to_bits().hash(&mut hash);
        }
    }
    sidecar.clicks.len().hash(&mut hash);
    for click in &sidecar.clicks {
        for value in [click.t, click.x, click.y] {
            value.to_bits().hash(&mut hash);
        }
    }
    sidecar.presses.len().hash(&mut hash);
    for press in &sidecar.presses {
        for value in [press.down, press.up] {
            value.to_bits().hash(&mut hash);
        }
        press.button.hash(&mut hash);
        press.dragged.hash(&mut hash);
    }
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PressSample,
    };

    fn ev(t: f64, x: f64, y: f64, kind: ZoomPathKind) -> ZoomPathEvent {
        ZoomPathEvent { t, x, y, kind }
    }

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
        )
    }

    fn move_at(sidecar: &mut PointerSidecar, t: f64, x: f64, y: f64) {
        sidecar.pointer.push(PointerSample {
            t,
            x,
            y,
            kind: CursorKind::Default,
            cursor_id: None,
        });
    }

    #[test]
    fn a_lone_spike_is_removed_before_any_zoom_sees_it() {
        let mut sidecar = sidecar();
        for (index, x) in [100.0, 105.0, 112.0, 130.0, 150.0].iter().enumerate() {
            move_at(&mut sidecar, index as f64 * 0.01, *x, 100.0);
        }
        move_at(&mut sidecar, 0.06, 1800.0, 900.0);
        move_at(&mut sidecar, 0.07, 152.0, 100.0);
        move_at(&mut sidecar, 0.08, 155.0, 100.0);

        let path = filter(&events(&sidecar, 1920.0, 1080.0), 500.0);

        assert!(
            path.iter().all(|event| event.x < 1000.0),
            "the teleport should be dropped, got {path:?}"
        );
    }

    #[test]
    fn decimation_keeps_clicks_and_releases_on_a_still_pointer() {
        let events = vec![
            ev(0.0, 100.0, 100.0, ZoomPathKind::Move),
            ev(0.01, 104.0, 100.0, ZoomPathKind::Move),
            ev(0.02, 108.0, 100.0, ZoomPathKind::Move),
            ev(0.03, 108.0, 100.0, ZoomPathKind::Down),
            ev(0.04, 108.0, 100.0, ZoomPathKind::Up),
            ev(0.05, 109.0, 100.0, ZoomPathKind::Move),
            ev(0.06, 125.0, 100.0, ZoomPathKind::Move),
        ];

        assert_eq!(
            decimate(&events, DECIMATE_DISTANCE),
            vec![
                ev(0.0, 100.0, 100.0, ZoomPathKind::Move),
                ev(0.03, 108.0, 100.0, ZoomPathKind::Down),
                ev(0.04, 108.0, 100.0, ZoomPathKind::Up),
                ev(0.06, 125.0, 100.0, ZoomPathKind::Move),
            ]
        );
    }

    #[test]
    fn a_zero_threshold_only_decimates() {
        let events = vec![
            ev(0.0, 100.0, 100.0, ZoomPathKind::Move),
            ev(0.01, 1800.0, 900.0, ZoomPathKind::Move),
            ev(0.02, 1801.0, 900.0, ZoomPathKind::Move),
        ];
        assert_eq!(filter(&events, 0.0).len(), 2);
        assert_eq!(filter(&events, f64::NAN).len(), 2);
    }

    #[test]
    fn events_put_moves_clicks_and_releases_in_time_order() {
        let mut sidecar = sidecar();
        move_at(&mut sidecar, 0.0, 100.0, 100.0);
        move_at(&mut sidecar, 1.0, 900.0, 500.0);
        sidecar.clicks.push(ClickSample {
            t: 0.5,
            x: 400.0,
            y: 300.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.presses.push(PressSample {
            down: 0.5,
            up: 1.0,
            button: 1,
            dragged: true,
        });

        let path = events(&sidecar, 1920.0, 1080.0);

        assert_eq!(
            path,
            vec![
                ev(0.0, 100.0, 100.0, ZoomPathKind::Move),
                ev(0.5, 400.0, 300.0, ZoomPathKind::Down),
                ev(1.0, 900.0, 500.0, ZoomPathKind::Move),
                ev(1.0, 900.0, 500.0, ZoomPathKind::Up),
            ]
        );
    }

    #[test]
    fn events_map_area_captures_into_video_pixels() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 960,
                h: 540,
            },
        );
        move_at(&mut sidecar, 0.0, 480.0, 270.0);

        let path = events(&sidecar, 1920.0, 1080.0);

        assert_eq!(path[0].x, 960.0);
        assert_eq!(path[0].y, 540.0);
    }

    #[test]
    fn the_cache_reuses_a_path_until_its_inputs_change() {
        let mut sidecar = sidecar();
        move_at(&mut sidecar, 0.0, 100.0, 100.0);
        let mut cache = ZoomPathCache::default();

        let first = cache.get_or_build(Some(&sidecar), 1920.0, 1080.0, 500.0);
        let again = cache.get_or_build(Some(&sidecar), 1920.0, 1080.0, 500.0);
        assert!(Arc::ptr_eq(&first, &again));

        let threshold = cache.get_or_build(Some(&sidecar), 1920.0, 1080.0, 0.0);
        assert!(
            !Arc::ptr_eq(&first, &threshold),
            "a new threshold refilters"
        );

        move_at(&mut sidecar, 0.5, 400.0, 400.0);
        let moved = cache.get_or_build(Some(&sidecar), 1920.0, 1080.0, 0.0);
        assert!(!Arc::ptr_eq(&threshold, &moved), "new samples rebuild");
        assert_eq!(moved.len(), 2);
    }
}
