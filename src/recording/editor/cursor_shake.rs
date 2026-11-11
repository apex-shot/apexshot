//! The studied shake filter.
//!
//! Before the reference plans its cursor path it drops pointer events that sit
//! far from both of the next two events: a lone sample that jumped away and
//! straight back, or the last sample before the pointer leapt across the
//! screen. A button-down that is dropped takes its button-up with it, so a
//! press never loses only one end. Distances are Manhattan distances in the
//! sidecar's pixel space.

/// The reference's default threshold, in pixels. Zero turns the filter off.
pub const DEFAULT_REMOVE_SHAKE_THRESHOLD: f64 = 500.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShakeEventKind {
    Move,
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShakeEvent {
    pub time_ms: f64,
    pub x: f64,
    pub y: f64,
    pub kind: ShakeEventKind,
}

/// Which of `events` (already in time order) survive the filter. The result
/// has one entry per event; every entry is `true` when the filter is off.
pub fn shake_keep_mask(events: &[ShakeEvent], threshold: f64) -> Vec<bool> {
    let mut keep = vec![true; events.len()];
    if !threshold.is_finite() || threshold <= 0.0 {
        return keep;
    }
    let far = |a: &ShakeEvent, b: &ShakeEvent| (a.x - b.x).abs() + (a.y - b.y).abs() > threshold;
    let mut drop_next_up = false;
    for (index, event) in events.iter().enumerate() {
        if drop_next_up && event.kind == ShakeEventKind::Up {
            drop_next_up = false;
            keep[index] = false;
            continue;
        }
        let (Some(next), Some(after)) = (events.get(index + 1), events.get(index + 2)) else {
            continue;
        };
        if far(event, next) && far(event, after) {
            keep[index] = false;
            if event.kind == ShakeEventKind::Down {
                drop_next_up = true;
            }
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(points: &[(f64, f64)]) -> Vec<ShakeEvent> {
        points
            .iter()
            .enumerate()
            .map(|(index, &(x, y))| ShakeEvent {
                time_ms: index as f64 * 10.0,
                x,
                y,
                kind: ShakeEventKind::Move,
            })
            .collect()
    }

    #[test]
    fn zero_threshold_keeps_everything() {
        let events = moves(&[(0.0, 0.0), (5000.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        assert_eq!(shake_keep_mask(&events, 0.0), vec![true; 4]);
        assert_eq!(shake_keep_mask(&events, f64::NAN), vec![true; 4]);
    }

    #[test]
    fn a_lone_spike_is_dropped() {
        let events = moves(&[(0.0, 0.0), (1000.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        assert_eq!(
            shake_keep_mask(&events, DEFAULT_REMOVE_SHAKE_THRESHOLD),
            vec![true, false, true, true]
        );
    }

    #[test]
    fn a_leap_keeps_its_destination_and_drops_the_last_sample_before_it() {
        let events = moves(&[(0.0, 0.0), (10.0, 0.0), (600.0, 0.0), (610.0, 0.0)]);
        assert_eq!(
            shake_keep_mask(&events, DEFAULT_REMOVE_SHAKE_THRESHOLD),
            vec![true, false, true, true]
        );
    }

    #[test]
    fn the_last_two_events_are_always_kept() {
        let events = moves(&[(0.0, 0.0), (1000.0, 0.0)]);
        assert_eq!(shake_keep_mask(&events, DEFAULT_REMOVE_SHAKE_THRESHOLD), vec![true, true]);
    }

    #[test]
    fn distance_is_manhattan() {
        // 300 + 300 = 600 > 500, although the straight-line distance is 424.
        let events = moves(&[(0.0, 0.0), (300.0, 300.0), (300.0, 300.0)]);
        assert_eq!(
            shake_keep_mask(&events, DEFAULT_REMOVE_SHAKE_THRESHOLD),
            vec![false, true, true]
        );
    }

    #[test]
    fn a_dropped_press_takes_its_release() {
        let event = |time_ms: f64, x: f64, kind| ShakeEvent {
            time_ms,
            x,
            y: 0.0,
            kind,
        };
        let events = vec![
            event(0.0, 0.0, ShakeEventKind::Move),
            event(10.0, 0.0, ShakeEventKind::Down),
            event(20.0, 800.0, ShakeEventKind::Move),
            event(30.0, 800.0, ShakeEventKind::Move),
            event(40.0, 800.0, ShakeEventKind::Up),
        ];
        assert_eq!(
            shake_keep_mask(&events, DEFAULT_REMOVE_SHAKE_THRESHOLD),
            vec![true, false, true, true, false]
        );
    }
}
