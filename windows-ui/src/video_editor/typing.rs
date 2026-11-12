//! Typing bursts and the typing state of a recording's key and pointer streams.
//!
//! A burst is a run of key downs whose consecutive gaps stay under half a
//! second; it qualifies when it lasts at least 1.5 seconds and averages at
//! least two downs a second. Typing at an instant needs three downs in the
//! preceding 1.8 seconds and no real pointer movement after the last of them.
//! Windows reports the pointer as periodic samples, so a stationary pointer
//! repeats the same position and only a changed position counts as movement.

use crate::recording::editor::sidecar::{KeySample, PointerSample};

/// The gap at or above which a burst ends, in seconds.
const BURST_GAP_SECONDS: f64 = 0.5;
/// The shortest qualifying burst, in seconds.
const BURST_MIN_SECONDS: f64 = 1.5;
/// The fewest downs a qualifying burst averages per second.
const BURST_MIN_RATE: f64 = 2.0;
/// How far back the typing window reaches, in seconds.
const TYPING_WINDOW_SECONDS: f64 = 1.8;
/// The downs a typing window needs.
const TYPING_MIN_DOWNS: usize = 3;
/// The pointer travel that counts as movement, in pixels.
const MOVEMENT_PIXELS: f64 = 0.5;

/// The bursts of key downs in `keys`, as first and last down times in
/// recording seconds. Only `key_down` samples with a finite non-negative time
/// are read, and same-time downs count as separate presses.
pub(super) fn detect(keys: &[KeySample]) -> Vec<(f64, f64)> {
    let mut times: Vec<f64> = keys
        .iter()
        .filter(|sample| sample.kind == "key_down" && sample.t.is_finite() && sample.t >= 0.0)
        .map(|sample| sample.t)
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let qualifies = |start: f64, end: f64, count: usize| {
        let duration = end - start;
        duration >= BURST_MIN_SECONDS && count as f64 / duration >= BURST_MIN_RATE
    };

    let mut ranges = Vec::new();
    let mut times = times.into_iter();
    let mut start = match times.next() {
        Some(first) => first,
        None => return ranges,
    };
    let mut end = start;
    let mut count = 1;
    for t in times {
        if t - end < BURST_GAP_SECONDS {
            end = t;
            count += 1;
        } else {
            if qualifies(start, end, count) {
                ranges.push((start, end));
            }
            start = t;
            end = t;
            count = 1;
        }
    }
    if qualifies(start, end, count) {
        ranges.push((start, end));
    }
    ranges
}

/// Whether `t` falls inside typing: at least three key downs in the 1.8 seconds
/// up to `t`, with no pointer movement after the last of them.
pub(super) fn is_typing_at(keys: &[KeySample], mouse: &[PointerSample], t: f64) -> bool {
    if !t.is_finite() || t < 0.0 {
        return false;
    }
    let window_start = (t - TYPING_WINDOW_SECONDS).max(0.0);

    let mut count = 0;
    let mut last_down = f64::NEG_INFINITY;
    for sample in keys {
        if sample.kind != "key_down" || !sample.t.is_finite() || sample.t < 0.0 {
            continue;
        }
        if sample.t >= window_start && sample.t <= t {
            count += 1;
            last_down = last_down.max(sample.t);
        }
    }
    if count < TYPING_MIN_DOWNS {
        return false;
    }

    let mut previous: Option<(f64, f64)> = None;
    for sample in mouse {
        if !sample.t.is_finite() || !sample.x.is_finite() || !sample.y.is_finite() {
            continue;
        }
        let moved = match previous {
            None => true,
            Some((x, y)) => (sample.x - x).hypot(sample.y - y) > MOVEMENT_PIXELS,
        };
        previous = Some((sample.x, sample.y));
        if moved && sample.t > last_down && sample.t <= t {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::CursorKind;

    fn down(t: f64) -> KeySample {
        KeySample {
            t,
            kind: "key_down".to_string(),
            key: 65,
            scan_code: 30,
            modifiers: Vec::new(),
            cursor_id: None,
        }
    }

    fn up(t: f64) -> KeySample {
        KeySample {
            kind: "key_up".to_string(),
            ..down(t)
        }
    }

    fn pointer(t: f64, x: f64, y: f64) -> PointerSample {
        PointerSample {
            t,
            x,
            y,
            kind: CursorKind::Default,
            cursor_id: None,
        }
    }

    fn downs(times: &[f64]) -> Vec<KeySample> {
        times.iter().map(|&t| down(t)).collect()
    }

    #[test]
    fn six_downs_in_one_burst() {
        assert_eq!(
            detect(&downs(&[0.0, 0.3, 0.6, 0.9, 1.2, 1.5])),
            vec![(0.0, 1.5)]
        );
    }

    #[test]
    fn burst_shorter_than_the_floor_does_not_qualify() {
        assert!(detect(&downs(&[0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.4])).is_empty());
    }

    #[test]
    fn wide_gaps_do_not_form_a_burst() {
        assert!(detect(&downs(&[0.0, 0.9, 1.8, 2.7, 3.6, 4.5])).is_empty());
    }

    #[test]
    fn exact_half_second_gap_splits_the_burst() {
        let keys = downs(&[0.0, 0.3, 0.6, 0.9, 1.2, 1.5, 2.0, 2.3, 2.6, 2.9, 3.2, 3.5]);
        assert_eq!(detect(&keys), vec![(0.0, 1.5), (2.0, 3.5)]);
    }

    #[test]
    fn key_ups_do_not_inflate_the_count() {
        let keys = vec![down(0.0), up(0.3), down(0.6), up(0.9), down(1.2), up(1.5)];
        assert!(detect(&keys).is_empty());
    }

    #[test]
    fn two_bursts_are_reported_separately() {
        let keys = downs(&[0.0, 0.3, 0.6, 0.9, 1.2, 1.5, 3.0, 3.3, 3.6, 3.9, 4.2, 4.5]);
        assert_eq!(detect(&keys), vec![(0.0, 1.5), (3.0, 4.5)]);
    }

    #[test]
    fn same_time_downs_count_but_a_single_instant_does_not_qualify() {
        assert!(detect(&downs(&[1.0, 1.0, 1.0, 1.0])).is_empty());
        let doubled = downs(&[0.0, 0.0, 0.3, 0.3, 0.6, 0.6, 0.9, 0.9, 1.2, 1.2, 1.5, 1.5]);
        assert_eq!(detect(&doubled), vec![(0.0, 1.5)]);
    }

    #[test]
    fn invalid_times_are_ignored() {
        let keys = vec![
            down(f64::NAN),
            down(-2.0),
            down(f64::INFINITY),
            down(0.0),
            down(0.3),
            down(0.6),
            down(0.9),
            down(1.2),
            down(1.5),
        ];
        assert_eq!(detect(&keys), vec![(0.0, 1.5)]);
        assert!(detect(&[down(f64::NAN), down(-1.0)]).is_empty());
    }

    #[test]
    fn three_downs_in_the_window_are_typing() {
        let keys = downs(&[4.0, 4.5, 4.9]);
        assert!(is_typing_at(&keys, &[], 5.0));
        assert!(is_typing_at(&keys, &[], 4.9));
        assert!(!is_typing_at(&keys, &[], 6.8));
    }

    #[test]
    fn too_few_downs_is_not_typing() {
        assert!(!is_typing_at(&downs(&[4.0, 4.5]), &[], 5.0));
        assert!(!is_typing_at(&[], &[], 5.0));
        assert!(!is_typing_at(&downs(&[1.0, 1.3, 1.6]), &[], 5.0));
    }

    #[test]
    fn key_ups_are_not_counted() {
        let keys = vec![down(4.0), up(4.1), down(4.3), up(4.4), up(4.6)];
        assert!(!is_typing_at(&keys, &[], 4.6));
    }

    #[test]
    fn invalid_instant_is_not_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        assert!(!is_typing_at(&keys, &[], f64::NAN));
        assert!(!is_typing_at(&keys, &[], -1.0));
    }

    #[test]
    fn movement_after_the_last_down_suppresses_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(3.0, 0.0, 0.0),
            pointer(4.0, 0.0, 0.0),
            pointer(4.7, 30.0, 40.0),
        ];
        assert!(!is_typing_at(&keys, &mouse, 5.0));
    }

    #[test]
    fn stationary_polling_samples_do_not_suppress_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(2.0, 10.0, 20.0),
            pointer(2.5, 10.0, 20.0),
            pointer(3.0, 10.0, 20.0),
            pointer(3.5, 10.0, 20.0),
            pointer(4.0, 10.0, 20.0),
            pointer(4.5, 10.0, 20.0),
            pointer(5.0, 10.0, 20.0),
        ];
        assert!(is_typing_at(&keys, &mouse, 5.0));
    }

    #[test]
    fn sub_pixel_jitter_is_not_movement() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(3.0, 10.0, 20.0),
            pointer(4.5, 10.3, 20.2),
            pointer(4.9, 10.0, 20.0),
        ];
        assert!(is_typing_at(&keys, &mouse, 5.0));
    }

    #[test]
    fn first_mouse_sample_counts_as_movement() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        assert!(!is_typing_at(&keys, &[pointer(4.7, 10.0, 20.0)], 5.0));
        assert!(is_typing_at(&keys, &[pointer(4.5, 10.0, 20.0)], 5.0));
    }

    #[test]
    fn movement_before_the_last_down_does_not_suppress_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![pointer(4.0, 0.0, 0.0), pointer(4.2, 90.0, 90.0)];
        assert!(is_typing_at(&keys, &mouse, 5.0));
    }

    #[test]
    fn future_mouse_movement_does_not_suppress_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![pointer(3.0, 0.0, 0.0), pointer(5.4, 300.0, 400.0)];
        assert!(is_typing_at(&keys, &mouse, 5.0));
    }

    #[test]
    fn invalid_mouse_samples_are_skipped() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(f64::NAN, 0.0, 0.0),
            pointer(4.7, f64::INFINITY, 0.0),
            pointer(3.0, 5.0, 5.0),
            pointer(4.8, 5.0, 5.0),
        ];
        assert!(is_typing_at(&keys, &mouse, 5.0));
        assert!(is_typing_at(
            &keys,
            &[
                pointer(f64::NAN, 0.0, 0.0),
                pointer(4.7, f64::INFINITY, 1.0)
            ],
            5.0
        ));
    }
}
