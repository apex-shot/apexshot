//! The cursor sprite's press scale.
//!
//! Mirrors the shared editor's `press_cursor_scale` spring: the sprite eases
//! toward `0.9` while a recorded press covers the playhead and back to `1.0`
//! after it releases, on a spring that stiffens for a drag and just before a
//! click. It is pure and deterministic — the ease restarts from the previous
//! held-state boundary, and overlapping presses are merged — so a random seek
//! lands where sequential playback would.

use super::zoom::{spring_step, CameraSpring, CAMERA_FOLLOW_SPRING};
use crate::recording::editor::sidecar::{PointerSidecar, PressSample};

/// How much the drawn cursor shrinks while a mouse button is held.
pub(super) const PRESSED_CURSOR_SCALE: f64 = 0.9;

/// The cursor-click spring: a click within the look-ahead window stiffens the
/// sprite so the press lands as the click does.
pub(super) const CURSOR_CLICK_SPRING: CameraSpring = CameraSpring {
    stiffness: 530.0,
    damping: 40.0,
    mass: 1.0,
};

/// How far ahead a click still stiffens the cursor spring.
pub(super) const CURSOR_CLICK_WINDOW_SECONDS: f64 = 0.175;

/// The cursor-drag spring: the drag base `{1000, 40, 1}` fit to settle in
/// 40 ms. The fit scales stiffness by `ratio^2` and damping by `ratio`, which
/// for the base's 379 ms settle is `ratio = 9.475`.
pub(super) const CURSOR_DRAG_SPRING: CameraSpring = CameraSpring {
    stiffness: 89775.625,
    damping: 379.0,
    mass: 1.0,
};

/// Which spring drives the cursor sprite at source time `t`: a recorded drag,
/// then an imminent click, then the follow spring.
pub(super) fn cursor_spring_for_time(
    click_times: &[f64],
    presses: &[PressSample],
    t: f64,
) -> CameraSpring {
    if !t.is_finite() {
        return CAMERA_FOLLOW_SPRING;
    }
    if presses
        .iter()
        .any(|press| press.dragged && press.contains(t))
    {
        return CURSOR_DRAG_SPRING;
    }
    let imminent = click_times
        .iter()
        .any(|click| click.is_finite() && *click >= t && *click - t <= CURSOR_CLICK_WINDOW_SECONDS);
    if imminent {
        CURSOR_CLICK_SPRING
    } else {
        CAMERA_FOLLOW_SPRING
    }
}

/// The cursor scale at source time `t`, eased toward the held-button scale.
pub(super) fn press_cursor_scale(sidecar: &PointerSidecar, t: f64) -> f64 {
    if !t.is_finite() {
        return 1.0;
    }
    let target = if sidecar.is_pressed_at(t) {
        PRESSED_CURSOR_SCALE
    } else {
        1.0
    };
    let Some((boundary, from)) = latest_press_boundary(&sidecar.presses, t) else {
        return target;
    };
    let click_times: Vec<f64> = sidecar.clicks.iter().map(|click| click.t).collect();
    let spring = cursor_spring_for_time(&click_times, &sidecar.presses, t);
    ease_scale(from, target, t - boundary, spring)
}

/// The latest held-state boundary at or before `t`, and the scale just before
/// it: `1.0` at a press start, [`PRESSED_CURSOR_SCALE`] at a release. Press
/// intervals are merged first so overlapping buttons step the target once.
fn latest_press_boundary(presses: &[PressSample], t: f64) -> Option<(f64, f64)> {
    let mut windows: Vec<(f64, f64)> = presses
        .iter()
        .filter(|press| press.down.is_finite() && press.up.is_finite() && press.up > press.down)
        .map(|press| (press.down, press.up))
        .collect();
    windows.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(windows.len());
    for (start, end) in windows {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut boundary = f64::NEG_INFINITY;
    let mut from = 1.0;
    let mut found = false;
    for (start, end) in merged {
        if start <= t && start > boundary {
            boundary = start;
            from = 1.0;
            found = true;
        }
        if end <= t && end > boundary {
            boundary = end;
            from = PRESSED_CURSOR_SCALE;
            found = true;
        }
    }
    found.then_some((boundary, from))
}

/// Integrate a scalar spring from `from` toward `target` over `seconds`,
/// stopping early once it has settled so a long hold is not re-integrated on
/// every frame.
fn ease_scale(from: f64, target: f64, seconds: f64, spring: CameraSpring) -> f64 {
    if !seconds.is_finite() || seconds <= 0.0 || (from - target).abs() < 1e-9 {
        return from;
    }
    let step = spring_adaptive_step_ms(spring) / 1000.0;
    if !step.is_finite() || step <= 0.0 {
        return target;
    }
    let max_steps = (seconds / step).ceil() as usize + 2;
    let mut value = from;
    let mut velocity: f64 = 0.0;
    let mut elapsed = 0.0;
    for _ in 0..max_steps {
        if elapsed >= seconds {
            break;
        }
        if (value - target).abs() < 1e-4 && velocity.abs() < 1e-4 {
            return target;
        }
        let dt = step.min(seconds - elapsed);
        let (next, next_velocity) = spring_step(value, velocity, target, spring, dt);
        value = next;
        velocity = next_velocity;
        elapsed += dt;
    }
    value
}

/// Adaptive integration step in milliseconds: the 1000/60 ms frame unless a
/// frame times the faster of the natural frequency and the damping rate
/// exceeds half a step, in which case 1 ms. The stiff cursor springs take the
/// 1 ms branch.
fn spring_adaptive_step_ms(spring: CameraSpring) -> f64 {
    let frame_ms = 1000.0 / 60.0;
    let mass = spring.mass.max(f64::EPSILON);
    let natural = (spring.stiffness / mass).sqrt();
    let damping_rate = spring.damping / mass;
    if frame_ms / 1000.0 * natural.max(damping_rate) <= 0.5 {
        frame_ms
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{press_cursor_scale, PRESSED_CURSOR_SCALE};
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, PointerSidecar, PressSample,
    };

    fn sidecar(presses: &[(f64, f64, bool)], clicks: &[f64]) -> PointerSidecar {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        );
        sidecar
            .presses
            .extend(presses.iter().map(|&(down, up, dragged)| PressSample {
                down,
                up,
                button: 1,
                dragged,
            }));
        sidecar.clicks.extend(clicks.iter().map(|&t| ClickSample {
            t,
            x: 0.0,
            y: 0.0,
            button: 1,
        }));
        sidecar
    }

    #[test]
    fn a_hold_reaches_the_pressed_scale_and_releases_to_full() {
        let sidecar = sidecar(&[(1.0, 1.4, false)], &[]);
        assert!(
            (press_cursor_scale(&sidecar, 1.35) - PRESSED_CURSOR_SCALE).abs() < 0.02,
            "{}",
            press_cursor_scale(&sidecar, 1.35)
        );
        assert!(
            (press_cursor_scale(&sidecar, 2.0) - 1.0).abs() < 1e-3,
            "{}",
            press_cursor_scale(&sidecar, 2.0)
        );
        assert_eq!(press_cursor_scale(&sidecar, 0.5), 1.0);
    }

    #[test]
    fn a_seek_inside_a_hold_matches_sequential_playback() {
        let sidecar = sidecar(&[(1.0, 1.4, false)], &[]);
        let expected = press_cursor_scale(&sidecar, 1.35);
        for earlier in [0.0, 0.9, 1.0, 1.1, 1.19] {
            let _ = press_cursor_scale(&sidecar, earlier);
        }
        assert_eq!(press_cursor_scale(&sidecar, 1.35), expected);
        assert!((expected - PRESSED_CURSOR_SCALE).abs() < 0.02);
    }

    #[test]
    fn a_drag_stiffens_the_approach_over_a_plain_click() {
        let plain = sidecar(&[(1.0, 1.4, false)], &[]);
        let drag = sidecar(&[(1.0, 1.4, true)], &[]);
        let plain_scale = press_cursor_scale(&plain, 1.05);
        let drag_scale = press_cursor_scale(&drag, 1.05);
        assert!(
            (drag_scale - PRESSED_CURSOR_SCALE).abs() < (plain_scale - PRESSED_CURSOR_SCALE).abs(),
            "drag {drag_scale} should lead plain {plain_scale}"
        );
    }

    #[test]
    fn overlapping_presses_behave_like_one_hold() {
        let overlapping = sidecar(&[(1.0, 1.4, false), (1.2, 1.6, false)], &[]);
        let merged = sidecar(&[(1.0, 1.6, false)], &[]);
        for time in [1.05, 1.2, 1.4, 1.5, 1.9, 2.2] {
            assert_eq!(
                press_cursor_scale(&overlapping, time),
                press_cursor_scale(&merged, time),
                "time={time}"
            );
        }
        assert!(
            (press_cursor_scale(&overlapping, 1.45) - PRESSED_CURSOR_SCALE).abs() < 0.01,
            "the released first button must not step the target back"
        );
    }
}
