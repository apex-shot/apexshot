//! Typing bursts and the typing state of a recording's key and pointer streams.
//!
//! A burst is a run of key events, downs and ups alike, whose consecutive gaps
//! stay under half a second; it qualifies when it lasts at least 1.5 seconds
//! and averages at least two events a second. Typing at an instant needs three
//! downs in the preceding 1.8 seconds and no real pointer movement after the
//! last key event of any kind. Windows reports the pointer as periodic samples,
//! so a stationary pointer repeats the same position and only a changed
//! position counts as movement. Modifier keys (Shift, Ctrl, Alt and the Windows
//! keys) carry no events in the studied key stream, so they are ignored.

use crate::recording::editor::sidecar::{KeySample, PointerSample, PressSample};

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

/// Virtual keys for Shift, Ctrl, Alt and the Windows keys, left, right and
/// generic. The studied key stream carries no events for them.
fn is_modifier(key: i32) -> bool {
    matches!(key, 16 | 17 | 18 | 91 | 92 | 160..=165)
}

/// The bursts of key events in `keys`, as first and last event times in
/// recording seconds. Both `key_down` and `key_up` samples with a finite
/// non-negative time are read, and same-time events count as separate presses.
pub(super) fn detect(keys: &[KeySample]) -> Vec<(f64, f64)> {
    let mut times: Vec<f64> = keys
        .iter()
        .filter(|sample| {
            (sample.kind == "key_down" || sample.kind == "key_up")
                && !is_modifier(sample.key)
                && sample.t.is_finite()
                && sample.t >= 0.0
        })
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
/// up to `t`, with no pointer movement after the last key event of any kind in
/// that window.
pub(super) fn is_typing_at(
    keys: &[KeySample],
    mouse: &[PointerSample],
    presses: &[PressSample],
    t: f64,
) -> bool {
    if !t.is_finite() || t < 0.0 {
        return false;
    }
    let window_start = (t - TYPING_WINDOW_SECONDS).max(0.0);

    let key_start = keys.partition_point(|sample| sample.t < window_start);
    let key_end = keys.partition_point(|sample| sample.t <= t);
    let mut count = 0;
    let mut last_event = f64::NEG_INFINITY;
    for sample in &keys[key_start..key_end] {
        if !sample.t.is_finite() || sample.t < 0.0 {
            continue;
        }
        if is_modifier(sample.key) {
            continue;
        }
        if sample.kind == "key_down" {
            count += 1;
        }
        last_event = last_event.max(sample.t);
    }
    if count < TYPING_MIN_DOWNS {
        return false;
    }

    let mut index = mouse
        .partition_point(|sample| sample.t < window_start)
        .saturating_sub(1);
    while index < mouse.len() {
        let sample = &mouse[index];
        if sample.t > t {
            break;
        }
        if !sample.t.is_finite() || !sample.x.is_finite() || !sample.y.is_finite() {
            index += 1;
            continue;
        }
        let moved = if index == 0 {
            true
        } else {
            let previous = &mouse[index - 1];
            (sample.x - previous.x).hypot(sample.y - previous.y) > MOVEMENT_PIXELS
        };
        if moved && sample.t > last_event && sample.t <= t {
            return false;
        }
        index += 1;
    }
    for press in presses {
        let down = press.down;
        let up = press.up;
        if (down.is_finite() && down > last_event && down <= t)
            || (up.is_finite() && up > last_event && up <= t)
        {
            return false;
        }
    }
    true
}

/// The studied cursor fades out on a stiff spring (stiffness 600, damping 50,
/// mass 1) when typing starts and back in on a softer one (340, 60, 3). Both
/// are close to critically damped, so they are drawn as critically damped
/// steps at their natural frequencies.
const FADE_OUT_OMEGA: f64 = 24.494_897_427_831_78; // sqrt(600 / 1)
const FADE_IN_OMEGA: f64 = 10.645_812_948_447_54; // sqrt(340 / 3)
/// How far back the fade looks; both springs have settled well inside it.
const FADE_HORIZON_SECONDS: f64 = 1.0;

fn settle(elapsed: f64, omega: f64) -> f64 {
    let x = (elapsed.max(0.0)) * omega;
    1.0 - (1.0 + x) * (-x).exp()
}

/// The cursor's opacity at `t` while typing hides it: 1 when not typing and
/// settled, 0 deep into typing, and a spring between them around each change.
pub(super) fn typing_alpha(
    keys: &[KeySample],
    mouse: &[PointerSample],
    presses: &[PressSample],
    t: f64,
) -> f64 {
    if !t.is_finite() || t < 0.0 {
        return 1.0;
    }
    let from = (t - FADE_HORIZON_SECONDS).max(0.0);

    let mut breakpoints: Vec<f64> = Vec::new();
    let key_start = keys.partition_point(|sample| sample.t <= from);
    let key_end = keys.partition_point(|sample| sample.t <= t);
    for sample in &keys[key_start..key_end] {
        if sample.t.is_finite() && sample.t >= 0.0 && !is_modifier(sample.key) {
            breakpoints.push(sample.t);
        }
    }
    let down_start = keys.partition_point(|sample| sample.t <= from - TYPING_WINDOW_SECONDS);
    let down_end = keys.partition_point(|sample| sample.t <= t - TYPING_WINDOW_SECONDS);
    for sample in &keys[down_start..down_end] {
        if sample.kind == "key_down"
            && sample.t.is_finite()
            && sample.t >= 0.0
            && !is_modifier(sample.key)
        {
            breakpoints.push(sample.t + TYPING_WINDOW_SECONDS);
        }
    }
    let mouse_start = mouse.partition_point(|sample| sample.t <= from);
    let mouse_end = mouse.partition_point(|sample| sample.t <= t);
    for sample in &mouse[mouse_start..mouse_end] {
        if sample.t.is_finite() && sample.t >= 0.0 {
            breakpoints.push(sample.t);
        }
    }
    for press in presses {
        for time in [press.down, press.up] {
            if time.is_finite() && time > from && time <= t {
                breakpoints.push(time);
            }
        }
    }
    breakpoints.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    breakpoints.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);

    let step = |alpha: f64, elapsed: f64, typing: bool| {
        if typing {
            alpha * (1.0 - settle(elapsed, FADE_OUT_OMEGA))
        } else {
            1.0 - (1.0 - alpha) * (1.0 - settle(elapsed, FADE_IN_OMEGA))
        }
    };

    let mut alpha = if is_typing_at(keys, mouse, presses, from) {
        0.0
    } else {
        1.0
    };
    let mut segment_start = from;
    for &breakpoint in &breakpoints {
        if breakpoint <= segment_start {
            continue;
        }
        let typing = is_typing_at(keys, mouse, presses, (segment_start + breakpoint) / 2.0);
        alpha = step(alpha, breakpoint - segment_start, typing);
        segment_start = breakpoint;
    }
    if t > segment_start {
        let typing = is_typing_at(keys, mouse, presses, t);
        alpha = step(alpha, t - segment_start, typing);
    }
    alpha.clamp(0.0, 1.0)
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

    fn key_event(t: f64, kind: &str, key: i32, modifiers: &[&str]) -> KeySample {
        KeySample {
            kind: kind.to_string(),
            key,
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
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
    fn key_ups_bridge_and_count_toward_a_burst() {
        let keys = vec![down(0.0), up(0.3), down(0.6), up(0.9), down(1.2), up(1.5)];
        assert_eq!(detect(&keys), vec![(0.0, 1.5)]);
    }

    #[test]
    fn downs_alone_too_slow_for_a_burst_qualify_with_their_ups() {
        let downs_only = downs(&[0.0, 0.6, 1.2, 1.8]);
        assert!(detect(&downs_only).is_empty());
        let keys = vec![
            down(0.0),
            up(0.3),
            down(0.6),
            up(0.9),
            down(1.2),
            up(1.5),
            down(1.8),
            up(2.1),
        ];
        assert_eq!(detect(&keys), vec![(0.0, 2.1)]);
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
        assert!(is_typing_at(&keys, &[], &[], 5.0));
        assert!(is_typing_at(&keys, &[], &[], 4.9));
        assert!(!is_typing_at(&keys, &[], &[], 6.8));
    }

    #[test]
    fn too_few_downs_is_not_typing() {
        assert!(!is_typing_at(&downs(&[4.0, 4.5]), &[], &[], 5.0));
        assert!(!is_typing_at(&[], &[], &[], 5.0));
        assert!(!is_typing_at(&downs(&[1.0, 1.3, 1.6]), &[], &[], 5.0));
    }

    #[test]
    fn key_ups_are_not_counted() {
        let keys = vec![down(4.0), up(4.1), down(4.3), up(4.4), up(4.6)];
        assert!(!is_typing_at(&keys, &[], &[], 4.6));
    }

    #[test]
    fn invalid_instant_is_not_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        assert!(!is_typing_at(&keys, &[], &[], f64::NAN));
        assert!(!is_typing_at(&keys, &[], &[], -1.0));
    }

    #[test]
    fn movement_after_the_last_down_suppresses_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(3.0, 0.0, 0.0),
            pointer(4.0, 0.0, 0.0),
            pointer(4.7, 30.0, 40.0),
        ];
        assert!(!is_typing_at(&keys, &mouse, &[], 5.0));
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
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
    }

    #[test]
    fn sub_pixel_jitter_is_not_movement() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![
            pointer(3.0, 10.0, 20.0),
            pointer(4.5, 10.3, 20.2),
            pointer(4.9, 10.0, 20.0),
        ];
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
    }

    #[test]
    fn first_mouse_sample_counts_as_movement() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        assert!(!is_typing_at(&keys, &[pointer(4.7, 10.0, 20.0)], &[], 5.0));
        assert!(is_typing_at(&keys, &[pointer(4.5, 10.0, 20.0)], &[], 5.0));
    }

    #[test]
    fn movement_before_the_last_down_does_not_suppress_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![pointer(4.0, 0.0, 0.0), pointer(4.2, 90.0, 90.0)];
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
    }

    #[test]
    fn movement_before_the_last_key_up_does_not_suppress_typing() {
        let keys = vec![down(4.0), down(4.3), down(4.6), up(4.8)];
        let mouse = vec![pointer(3.0, 0.0, 0.0), pointer(4.7, 30.0, 40.0)];
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
    }

    #[test]
    fn future_mouse_movement_does_not_suppress_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let mouse = vec![pointer(3.0, 0.0, 0.0), pointer(5.4, 300.0, 400.0)];
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
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
        assert!(is_typing_at(&keys, &mouse, &[], 5.0));
        assert!(is_typing_at(
            &keys,
            &[
                pointer(f64::NAN, 0.0, 0.0),
                pointer(4.7, f64::INFINITY, 1.0)
            ],
            &[],
            5.0
        ));
    }

    #[test]
    fn the_cursor_is_opaque_without_typing() {
        assert_eq!(typing_alpha(&[], &[], &[], 3.0), 1.0);
    }

    #[test]
    fn the_cursor_fades_out_quickly_once_typing_starts() {
        let keys = downs(&[2.0, 2.1, 2.2]);
        assert!(typing_alpha(&keys, &[], &[], 2.2) > 0.95);
        let midway = typing_alpha(&keys, &[], &[], 2.25);
        assert!(midway > 0.05 && midway < 0.95);
        assert!(typing_alpha(&keys, &[], &[], 2.6) < 0.01);
    }

    #[test]
    fn the_cursor_fades_back_in_after_typing() {
        let keys = downs(&[2.0, 2.1, 2.2]);
        let mouse = vec![pointer(1.0, 0.0, 0.0), pointer(3.0, 50.0, 50.0)];
        assert!(typing_alpha(&keys, &mouse, &[], 3.0) < 0.05);
        let midway = typing_alpha(&keys, &mouse, &[], 3.15);
        assert!(midway > 0.05 && midway < 0.95);
        assert!(typing_alpha(&keys, &mouse, &[], 3.9) > 0.97);
    }

    #[test]
    fn typing_alpha_is_continuous() {
        let keys = downs(&[2.0, 2.1, 2.2]);
        let mouse = vec![pointer(1.0, 0.0, 0.0), pointer(3.0, 50.0, 50.0)];
        let mut previous = typing_alpha(&keys, &mouse, &[], 1.9);
        let mut t = 1.9;
        while t <= 4.0 {
            t += 0.005;
            let alpha = typing_alpha(&keys, &mouse, &[], t);
            assert!((alpha - previous).abs() < 0.2);
            previous = alpha;
        }
    }

    #[test]
    fn a_shortcut_alone_is_not_typing() {
        let keys = vec![
            key_event(6.90, "key_down", 162, &["control"]),
            key_event(6.95, "key_down", 160, &["control"]),
            key_event(7.00, "key_down", 75, &["control", "shift"]),
            key_event(7.06, "key_up", 75, &["control", "shift"]),
            key_event(7.10, "key_up", 160, &["control"]),
            key_event(7.15, "key_up", 162, &["control"]),
        ];
        assert!(!is_typing_at(&keys, &[], &[], 7.5));
    }

    #[test]
    fn modifier_downs_do_not_make_a_burst() {
        let keys: Vec<KeySample> = (0..=10)
            .map(|step| key_event(f64::from(step) * 0.2, "key_down", 16, &[]))
            .collect();
        assert!(detect(&keys).is_empty());
    }

    #[test]
    fn a_click_after_the_last_key_ends_typing() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let presses = [PressSample {
            down: 4.8,
            up: 4.9,
            button: 1,
            dragged: false,
        }];
        assert!(!is_typing_at(&keys, &[], &presses, 5.0));
        let early = [PressSample {
            down: 4.5,
            up: 4.55,
            button: 1,
            dragged: false,
        }];
        assert!(is_typing_at(&keys, &[], &early, 5.0));
    }

    #[test]
    fn a_click_brings_the_cursor_back() {
        let keys = downs(&[4.0, 4.3, 4.6]);
        let presses = [PressSample {
            down: 5.0,
            up: 5.08,
            button: 1,
            dragged: false,
        }];
        assert!(typing_alpha(&keys, &[], &presses, 4.95) < 0.05);
        assert!(typing_alpha(&keys, &[], &presses, 5.6) > 0.9);
    }
}
