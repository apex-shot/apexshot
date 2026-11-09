//! The studied build's pointer-path filters, applied before the camera reads
//! the path: shake removal (`B39:2846-2862`, default threshold from
//! `DgH:3254`) and distance decimation that always keeps clicks
//! (`B39:2804-2826`).

/// The studied default for removing cursor shakes, in pixels of Manhattan distance.
pub(super) const SHAKE_THRESHOLD: f64 = 500.0;
/// The studied minimum distance between kept pointer moves, in pixels.
pub(super) const DECIMATE_DISTANCE: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointerKind {
    Move,
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PointerEvent {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub kind: PointerKind,
}

/// The studied shake filter (`B39:2846-2862`): drop an event when both of the
/// next two events are more than `threshold` away in Manhattan distance. A
/// press dropped this way also drops the next release.
pub(super) fn remove_shakes(events: &[PointerEvent], threshold: f64) -> Vec<PointerEvent> {
    if !threshold.is_finite() || threshold <= 0.0 {
        return events.to_vec();
    }

    fn far(a: PointerEvent, b: PointerEvent, threshold: f64) -> bool {
        (a.x - b.x).abs() + (a.y - b.y).abs() > threshold
    }

    let mut kept = Vec::with_capacity(events.len());
    let mut dropping_press = false;
    for (i, event) in events.iter().copied().enumerate() {
        if dropping_press && event.kind == PointerKind::Up {
            dropping_press = false;
            continue;
        }

        let Some(next) = events.get(i + 1) else {
            kept.push(event);
            continue;
        };
        if !far(event, *next, threshold) {
            kept.push(event);
            continue;
        }

        let Some(after_next) = events.get(i + 2) else {
            kept.push(event);
            continue;
        };
        if !far(event, *after_next, threshold) {
            kept.push(event);
            continue;
        }

        if event.kind == PointerKind::Down {
            dropping_press = true;
        }
    }
    kept
}

/// The studied decimation (`B39:2804-2826`): presses and releases are always
/// kept; a move is kept once it is `min_distance` from the last kept event.
pub(super) fn decimate(events: &[PointerEvent], min_distance: f64) -> Vec<PointerEvent> {
    let mut kept = Vec::with_capacity(events.len());
    let Some((first, rest)) = events.split_first() else {
        return kept;
    };
    kept.push(*first);
    let mut last = *first;
    for event in rest {
        let keep = match event.kind {
            PointerKind::Down | PointerKind::Up => true,
            PointerKind::Move => {
                let dx = event.x - last.x;
                let dy = event.y - last.y;
                (dx * dx + dy * dy).sqrt() >= min_distance
            }
        };
        if keep {
            kept.push(*event);
            last = *event;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::{decimate, remove_shakes, PointerEvent, PointerKind};

    fn ev(t: f64, x: f64, y: f64, kind: PointerKind) -> PointerEvent {
        PointerEvent { t, x, y, kind }
    }

    #[test]
    fn a_lone_spike_is_removed() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 105.0, 100.0, PointerKind::Move),
            ev(20.0, 112.0, 100.0, PointerKind::Move),
            ev(30.0, 130.0, 100.0, PointerKind::Move),
            ev(40.0, 150.0, 100.0, PointerKind::Move),
            ev(50.0, 900.0, 900.0, PointerKind::Move),
            ev(60.0, 152.0, 100.0, PointerKind::Move),
            ev(70.0, 155.0, 100.0, PointerKind::Move),
        ];

        let kept = remove_shakes(&events, 500.0);

        let expected: Vec<PointerEvent> = events.iter().copied().filter(|e| e.t != 50.0).collect();
        assert_eq!(kept, expected);
    }

    #[test]
    fn a_far_press_drops_with_its_release() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 101.0, 100.0, PointerKind::Move),
            ev(20.0, 900.0, 900.0, PointerKind::Down),
            ev(25.0, 900.0, 900.0, PointerKind::Up),
            ev(30.0, 102.0, 100.0, PointerKind::Move),
            ev(40.0, 103.0, 100.0, PointerKind::Move),
        ];

        let kept = remove_shakes(&events, 500.0);

        // The move before the jump is dropped because both of its next two
        // events are far, and the release is dropped for the same reason, so
        // `dropping_press` never engages here.
        // matches the studied rule
        assert_eq!(
            kept,
            vec![
                ev(0.0, 100.0, 100.0, PointerKind::Move),
                ev(20.0, 900.0, 900.0, PointerKind::Down),
                ev(30.0, 102.0, 100.0, PointerKind::Move),
                ev(40.0, 103.0, 100.0, PointerKind::Move),
            ]
        );
    }

    #[test]
    fn the_event_before_a_jump_is_dropped_by_the_studied_rule() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 101.0, 100.0, PointerKind::Move),
            ev(20.0, 900.0, 900.0, PointerKind::Move),
            ev(30.0, 901.0, 900.0, PointerKind::Move),
            ev(40.0, 902.0, 900.0, PointerKind::Move),
        ];

        let kept = remove_shakes(&events, 500.0);

        assert_eq!(
            kept,
            vec![
                ev(0.0, 100.0, 100.0, PointerKind::Move),
                ev(20.0, 900.0, 900.0, PointerKind::Move),
                ev(30.0, 901.0, 900.0, PointerKind::Move),
                ev(40.0, 902.0, 900.0, PointerKind::Move),
            ]
        );
    }

    #[test]
    fn decimation_keeps_clicks_on_a_still_pointer() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 104.0, 100.0, PointerKind::Move),
            ev(20.0, 108.0, 100.0, PointerKind::Move),
            ev(30.0, 108.0, 100.0, PointerKind::Down),
            ev(40.0, 108.0, 100.0, PointerKind::Up),
            ev(50.0, 109.0, 100.0, PointerKind::Move),
            ev(60.0, 125.0, 100.0, PointerKind::Move),
        ];

        let kept = decimate(&events, 10.0);

        assert_eq!(
            kept,
            vec![
                ev(0.0, 100.0, 100.0, PointerKind::Move),
                ev(30.0, 108.0, 100.0, PointerKind::Down),
                ev(40.0, 108.0, 100.0, PointerKind::Up),
                ev(60.0, 125.0, 100.0, PointerKind::Move),
            ]
        );
    }

    #[test]
    fn zero_threshold_keeps_everything() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 900.0, 900.0, PointerKind::Move),
            ev(20.0, 901.0, 900.0, PointerKind::Move),
            ev(30.0, 900.0, 900.0, PointerKind::Down),
        ];

        assert_eq!(remove_shakes(&events, 0.0), events);
    }

    #[test]
    fn non_finite_threshold_keeps_everything() {
        let events = vec![
            ev(0.0, 100.0, 100.0, PointerKind::Move),
            ev(10.0, 900.0, 900.0, PointerKind::Move),
            ev(20.0, 901.0, 900.0, PointerKind::Move),
        ];

        assert_eq!(remove_shakes(&events, f64::NAN), events);
        assert_eq!(remove_shakes(&events, f64::INFINITY), events);
    }
}
