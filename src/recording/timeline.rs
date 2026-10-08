//! Map the compositor's monotonic input clock to the retained video timeline.

use super::editor::sidecar::PointerSidecar;

#[derive(Debug, Default)]
pub(super) struct RecordingTimeline {
    origin_us: Option<i64>,
    end_us: Option<i64>,
    pauses: Vec<(i64, i64)>,
    paused_at: Option<i64>,
    keep_pause_duration: bool,
}

impl RecordingTimeline {
    pub(super) fn retain_live_audio_pauses(&mut self) {
        self.keep_pause_duration = true;
    }

    pub(super) fn frame_time_us(&mut self, now_us: i64) -> u64 {
        self.origin_us.get_or_insert(now_us);
        self.elapsed_us(now_us) as u64
    }

    pub(super) fn pause(&mut self, now_us: i64) {
        if self.paused_at.is_none() {
            self.paused_at = Some(now_us);
        }
    }

    pub(super) fn resume(&mut self, now_us: i64) {
        if let Some(start) = self.paused_at.take() {
            self.pauses.push((start, now_us.max(start)));
        }
    }

    pub(super) fn finish(&mut self, end_us: i64) {
        self.resume(end_us);
        self.end_us = Some(end_us);
    }

    fn elapsed_us(&self, at_us: i64) -> i64 {
        let origin = self.origin_us.unwrap_or(at_us);
        if self.keep_pause_duration {
            let boundary = self
                .pauses
                .iter()
                .find_map(|&(start, end)| (at_us >= start && at_us < end).then_some(end))
                .unwrap_or(at_us);
            return (boundary - origin).max(0);
        }
        let paused: i64 = self
            .pauses
            .iter()
            .map(|&(start, end)| (at_us.min(end) - origin.max(start)).max(0))
            .sum();
        (at_us - origin - paused).max(0)
    }

    pub(super) fn rebase(&self, sidecar: &mut PointerSidecar) -> bool {
        let (Some(origin), Some(end)) = (self.origin_us, self.end_us) else {
            return false;
        };
        let old_origin = sidecar.t0_monotonic_us;
        if old_origin <= 0 || end <= origin {
            return false;
        }
        let absolute = |t: f64| old_origin.saturating_add((t * 1_000_000.0).round() as i64);
        let mapped = |at: i64| self.elapsed_us(at) as f64 / 1_000_000.0;
        let mut spans = Vec::with_capacity(self.pauses.len() + 1);
        let mut start = origin;
        for &(pause, resume) in &self.pauses {
            if pause > start {
                spans.push((start, pause.min(end)));
            }
            start = start.max(resume);
        }
        if start < end {
            spans.push((start, end));
        }
        spans.retain(|&(start, end)| start < end);
        let mut pointer = Vec::new();
        for &(start, end) in &spans {
            if self.keep_pause_duration {
                if let Some(previous) = pointer.last().cloned() {
                    let mut hold: super::editor::sidecar::PointerSample = previous;
                    hold.t = mapped(start) - 0.000001;
                    if hold.t >= 0.0 {
                        pointer.push(hold);
                    }
                }
            }
            if let Some(sample) = sidecar
                .pointer
                .iter()
                .rev()
                .find(|sample| sample.t.is_finite() && absolute(sample.t) <= start)
            {
                let mut sample = sample.clone();
                sample.t = mapped(start);
                pointer.push(sample);
            }
            for sample in &sidecar.pointer {
                let at = absolute(sample.t);
                if sample.t.is_finite() && at > start && at < end {
                    let mut sample = sample.clone();
                    sample.t = mapped(at);
                    pointer.push(sample);
                }
            }
        }
        sidecar.pointer = pointer;
        sidecar.clicks.retain_mut(|click| {
            let at = absolute(click.t);
            if !click.t.is_finite() || !spans.iter().any(|&(start, end)| at >= start && at < end) {
                return false;
            }
            click.t = mapped(at);
            true
        });
        sidecar.presses.retain_mut(|press| {
            if !press.down.is_finite() || !press.up.is_finite() || press.up <= press.down {
                return false;
            }
            let down = absolute(press.down).max(origin);
            let up = absolute(press.up).min(end);
            if !spans
                .iter()
                .any(|&(start, end)| down.max(start) < up.min(end))
            {
                return false;
            }
            press.down = mapped(down);
            press.up = mapped(up);
            press.up > press.down
        });
        sidecar.t0_monotonic_us = origin;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PressSample,
    };

    fn pointer(t: f64, x: f64) -> PointerSample {
        PointerSample {
            t,
            x,
            y: 20.0,
            kind: CursorKind::Default,
        }
    }

    fn click(t: f64) -> ClickSample {
        ClickSample {
            t,
            x: 10.0,
            y: 20.0,
            button: 1,
        }
    }

    fn press(down: f64, up: f64) -> PressSample {
        PressSample {
            down,
            up,
            button: 1,
            dragged: true,
        }
    }

    #[test]
    fn first_retained_frame_rebases_every_pointer_event_without_a_fixed_offset() {
        for delay in [137_000, 1_137_000, 2_500_000] {
            let mut clock = RecordingTimeline::default();
            assert_eq!(clock.frame_time_us(1_000_000 + delay), 0);
            clock.finish(6_000_000 + delay);
            let mut sidecar = PointerSidecar::new(
                1_000_000,
                CaptureRegion::from_capture(None, None, None, None),
            );
            let offset = delay as f64 / 1_000_000.0;
            sidecar.pointer = vec![pointer(0.0, 5.0), pointer(offset + 1.0, 10.0)];
            sidecar.clicks = vec![click(0.0), click(offset + 1.0), click(offset + 6.0)];
            sidecar.presses = vec![press(0.0, offset + 0.5), press(offset + 1.0, offset + 2.0)];
            assert!(clock.rebase(&mut sidecar));
            assert_eq!(sidecar.t0_monotonic_us, 1_000_000 + delay);
            assert_eq!(sidecar.pointer, vec![pointer(0.0, 5.0), pointer(1.0, 10.0)]);
            assert_eq!(sidecar.clicks, vec![click(1.0)]);
            assert_eq!(sidecar.presses, vec![press(0.0, 0.5), press(1.0, 2.0)]);
        }
    }

    #[test]
    fn pauses_remove_hidden_events_and_rebase_both_ends_of_held_presses() {
        let mut clock = RecordingTimeline::default();
        clock.frame_time_us(2_000_000);
        clock.pause(4_000_000);
        clock.pause(5_000_000);
        clock.resume(7_000_000);
        clock.resume(8_000_000);
        assert_eq!(clock.frame_time_us(8_000_000), 3_000_000);
        clock.finish(10_000_000);
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        sidecar.pointer = vec![
            pointer(0.0, 0.0),
            pointer(2.0, 2.0),
            pointer(5.0, 5.0),
            pointer(7.0, 7.0),
        ];
        sidecar.clicks = vec![click(2.0), click(3.0), click(4.0), click(6.0), click(9.0)];
        sidecar.presses = vec![
            press(2.0, 7.0),
            press(4.0, 5.0),
            press(5.0, 7.0),
            press(2.0, 4.0),
        ];
        assert!(clock.rebase(&mut sidecar));
        assert_eq!(
            sidecar.pointer,
            vec![
                pointer(0.0, 0.0),
                pointer(1.0, 2.0),
                pointer(2.0, 5.0),
                pointer(3.0, 7.0)
            ]
        );
        assert_eq!(sidecar.clicks, vec![click(1.0), click(2.0)]);
        assert_eq!(
            sidecar.presses,
            vec![press(1.0, 3.0), press(2.0, 3.0), press(1.0, 2.0)]
        );
    }

    #[test]
    fn stopping_while_paused_keeps_only_the_active_recording() {
        let mut clock = RecordingTimeline::default();
        clock.frame_time_us(2_000_000);
        clock.pause(4_000_000);
        clock.finish(8_000_000);
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        sidecar.clicks = vec![click(2.0), click(5.0)];
        sidecar.presses = vec![press(2.0, 6.0)];
        assert!(clock.rebase(&mut sidecar));
        assert_eq!(sidecar.clicks, vec![click(1.0)]);
        assert_eq!(sidecar.presses, vec![press(1.0, 2.0)]);
    }

    #[test]
    fn static_video_tail_uses_the_active_stop_clock_instead_of_the_last_changed_frame() {
        let mut clock = RecordingTimeline::default();
        assert_eq!(clock.frame_time_us(2_000_000), 0);
        clock.pause(6_000_000);
        clock.resume(10_000_000);
        clock.finish(22_000_000);
        assert_eq!(clock.frame_time_us(22_000_000), 16_000_000);
        assert_eq!(clock.frame_time_us(2_000_000), 0);
        let mut stopped_paused = RecordingTimeline::default();
        stopped_paused.frame_time_us(2_000_000);
        stopped_paused.pause(6_000_000);
        stopped_paused.finish(22_000_000);
        assert_eq!(stopped_paused.frame_time_us(22_000_000), 4_000_000);
    }

    #[test]
    fn live_audio_fallback_keeps_frozen_pause_time_without_replaying_hidden_interactions() {
        let mut clock = RecordingTimeline::default();
        clock.retain_live_audio_pauses();
        clock.frame_time_us(2_000_000);
        clock.pause(4_000_000);
        clock.resume(7_000_000);
        assert_eq!(clock.frame_time_us(6_000_000), 5_000_000);
        assert_eq!(clock.frame_time_us(8_000_000), 6_000_000);
        clock.finish(10_000_000);
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        sidecar.pointer = vec![
            pointer(0.0, 0.0),
            pointer(2.0, 2.0),
            pointer(5.0, 5.0),
            pointer(7.0, 7.0),
        ];
        sidecar.clicks = vec![click(2.0), click(4.0), click(6.0)];
        sidecar.presses = vec![
            press(2.0, 7.0),
            press(4.0, 5.0),
            press(5.0, 7.0),
            press(2.0, 4.0),
        ];
        assert!(clock.rebase(&mut sidecar));
        assert_eq!(sidecar.clicks, vec![click(1.0), click(5.0)]);
        assert_eq!(
            sidecar.presses,
            vec![press(1.0, 6.0), press(5.0, 6.0), press(1.0, 5.0)]
        );
        assert_eq!(sidecar.interpolated_at(4.5).unwrap().0, 2.0);
        assert_eq!(sidecar.interpolated_at(5.0).unwrap().0, 5.0);
    }

    #[test]
    fn empty_cancelled_and_restarted_clocks_do_not_reuse_a_previous_origin() {
        let mut old = RecordingTimeline::default();
        old.frame_time_us(2_000_000);
        old.finish(3_000_000);
        let mut new = RecordingTimeline::default();
        let mut sidecar = PointerSidecar::new(
            4_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        assert!(!new.rebase(&mut sidecar));
        assert_eq!(new.frame_time_us(5_000_000), 0);
        new.finish(6_000_000);
        sidecar.clicks = vec![click(1.5)];
        sidecar.presses = Vec::new();
        assert!(new.rebase(&mut sidecar));
        assert_eq!(sidecar.clicks, vec![click(0.5)]);
        assert!(sidecar.presses.is_empty());
    }

    #[test]
    fn rebased_cursor_press_ripple_and_automatic_zoom_use_the_same_source_time() {
        use crate::recording::editor::{auto_zoom, sidecar::CursorMotion};

        let mut clock = RecordingTimeline::default();
        clock.frame_time_us(2_000_000);
        clock.pause(4_000_000);
        clock.resume(6_000_000);
        assert_eq!(clock.frame_time_us(8_000_000), 4_000_000);
        clock.finish(12_000_000);
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 1280,
                h: 800,
            },
        );
        sidecar.pointer = vec![PointerSample {
            t: 0.0,
            x: 320.0,
            y: 200.0,
            kind: CursorKind::Hand,
        }];
        sidecar.clicks = vec![ClickSample {
            t: 7.0,
            x: 320.0,
            y: 200.0,
            button: 1,
        }];
        sidecar.presses = vec![press(7.0, 7.5)];
        assert!(clock.rebase(&mut sidecar));
        let cursor = sidecar.presented_at(4.0, CursorMotion::default()).unwrap();
        assert_eq!(
            (cursor.x, cursor.y, cursor.kind),
            (320.0, 200.0, CursorKind::Hand)
        );
        assert!(!sidecar.is_pressed_at(3.99));
        assert!(sidecar.is_pressed_at(4.0));
        assert!(!sidecar.is_pressed_at(4.5));
        assert_eq!(sidecar.click_ripples_at(4.1, 0.8).len(), 1);
        assert!(sidecar.click_ripples_at(3.9, 0.8).is_empty());
        let zooms = auto_zoom::automatic_zooms(
            &auto_zoom::clicks_from_sidecar(&sidecar, 1280.0, 800.0),
            &auto_zoom::pointer_from_sidecar(&sidecar, 1280.0, 800.0),
            8.0,
        );
        assert_eq!(zooms.len(), 1);
        assert_eq!(zooms[0].center_time, 4.0);
        assert_eq!(zooms[0].start, 4.0 - auto_zoom::LEAD_SECONDS);
        assert_eq!(zooms[0].center, (320.0, 200.0));
    }
}
