pub fn near_playhead(state: &VideoEditState, width: f64, x: f64) -> bool {
    (x - state.time_to_x(state.playhead_seconds, width)).abs() <= PLAYHEAD_HIT
}

pub fn video_layout(state: &VideoEditState, width: f64) -> Vec<(usize, usize, f64, f64)> {
    let bounds = state.segment_boundaries();
    let mut layout = Vec::new();
    for (order_pos, &seg_idx) in state.segment_order.iter().enumerate() {
        if !state.segments_kept.get(seg_idx).copied().unwrap_or(true) {
            continue;
        }
        if bounds.get(seg_idx).is_none() {
            continue;
        }
        let comp = state.segment_start(seg_idx);
        let x0 = state.time_to_x(comp, width);
        // The final segment's hit box covers its freeze hold, so the right
        // handle stays grabbable once the clip has been held open.
        let hold = if state.freeze_applies_to_segment(seg_idx) {
            state.freeze_tail_seconds()
        } else {
            0.0
        };
        let x1 = state.time_to_x(
            comp + state.segment_timeline_duration(seg_idx) + hold,
            width,
        );
        layout.push((order_pos, seg_idx, x0, x1.max(x0 + 8.0)));
    }
    layout
}

/// Right edge (x) of the drawn clip on the video lane, including any freeze
/// hold. Everything from here to the lane's edge is the "extend duration"
/// band, which is what the blurred affordance fills.
pub fn video_end_x(state: &VideoEditState, width: f64) -> f64 {
    video_layout(state, width)
        .into_iter()
        .map(|(_, _, _, x1)| x1)
        .fold(0.0_f64, f64::max)
}

pub struct VideoHit {
    pub cursor: TrackCursor,
    pub segment: Option<usize>,
    pub drag: Option<ClipDrag>,
}

pub fn video_hit(state: &VideoEditState, width: f64, x: f64) -> VideoHit {
    let layout = video_layout(state, width);
    let end_x = layout
        .iter()
        .map(|&(_, _, _, x1)| x1)
        .fold(0.0_f64, f64::max);
    // The mover straddles the clip's right edge, so its hit region starts just
    // before `end_x` and runs through the band. Claiming it first makes the
    // handle the drag target even where the last segment's body or its inset
    // edge handle would otherwise win.
    if end_x > 0.0 && x >= end_x - EXTEND_HANDLE_HIT {
        let segment = rightmost_video_segment(state, width);
        return VideoHit {
            cursor: TrackCursor::ResizeEnd,
            segment,
            drag: segment.and_then(|index| segment_edge_drag(state, index, false)),
        };
    }
    for &(_, seg_idx, x0, x1) in &layout {
        if x < x0 || x > x1 {
            continue;
        }
        let (cursor, drag) = match clip_edge_at(x, x0, x1) {
            Some(true) => (
                TrackCursor::ResizeStart,
                segment_edge_drag(state, seg_idx, true),
            ),
            Some(false) => (
                TrackCursor::ResizeEnd,
                segment_edge_drag(state, seg_idx, false),
            ),
            None if state.cuts.is_empty() && state.segment_order.len() > 1 => (
                TrackCursor::Grab,
                Some(ClipDrag::Move {
                    origin_offset: state.timeline_offset_seconds,
                    pixels_per_second: pixels_per_second(state, width),
                }),
            ),
            // The body of a lone clip is not a drag target: sliding it would
            // only open a gap the player skips over. A cut arrangement is the
            // case where moving a clip means something.
            None if state.segment_order.len() <= 1 => (TrackCursor::None, None),
            None => (
                TrackCursor::Grab,
                Some(ClipDrag::Segment {
                    index: seg_idx,
                    origin_start: state.segment_start(seg_idx),
                    pixels_per_second: pixels_per_second(state, width),
                }),
            ),
        };
        return VideoHit {
            cursor,
            segment: Some(seg_idx),
            drag,
        };
    }
    VideoHit {
        cursor: TrackCursor::None,
        segment: None,
        drag: None,
    }
}

pub fn segment_edge_drag(state: &VideoEditState, seg_idx: usize, is_start: bool) -> Option<ClipDrag> {
    let Some(&(start, end)) = state.segment_boundaries().get(seg_idx) else {
        return Some(if is_start {
            ClipDrag::Start
        } else {
            ClipDrag::End
        });
    };
    let edge = if is_start { start } else { end };
    if is_start && (edge - state.trim_start_seconds).abs() < 1e-3 {
        return Some(ClipDrag::Start);
    }
    if !is_start && (edge - state.trim_end_seconds).abs() < 1e-3 {
        return Some(ClipDrag::End);
    }
    state
        .cuts
        .iter()
        .position(|cut| (*cut - edge).abs() < 1e-3)
        .map(ClipDrag::Cut)
}

pub fn select_video(state: &mut VideoEditState, segment: Option<usize>) {
    state.selected_segment = segment;
    if segment.is_some() {
        state.selected_zoom = None;
        state.selected_cursor_hide = None;
        state.selected_tool = crate::recording::editor::model::EditorTool::Timeline;
    }
}

pub fn select_zoom(state: &mut VideoEditState, index: Option<usize>) {
    state.selected_zoom = index;
    if index.is_some() {
        state.selected_segment = None;
        state.selected_cursor_hide = None;
        state.selected_tool = crate::recording::editor::model::EditorTool::Timeline;
    }
}

pub fn select_cursor_hide(state: &mut VideoEditState, index: Option<usize>) {
    state.selected_cursor_hide = index;
    if index.is_some() {
        state.selected_segment = None;
        state.selected_zoom = None;
        state.selected_tool = crate::recording::editor::model::EditorTool::Timeline;
    }
}

pub fn clip_edge_at(x: f64, start_x: f64, end_x: f64) -> Option<bool> {
    let left = start_x + HANDLE_INSET;
    let right = end_x - HANDLE_INSET - HANDLE_WIDTH;
    if x >= left - HANDLE_HIT && x <= left + HANDLE_WIDTH + HANDLE_HIT {
        Some(true)
    } else if x >= right - HANDLE_HIT && x <= right + HANDLE_WIDTH + HANDLE_HIT {
        Some(false)
    } else {
        None
    }
}

pub fn zoom_edge_at(state: &VideoEditState, width: f64, x: f64) -> Option<(usize, bool)> {
    state
        .zoom_clips
        .iter()
        .enumerate()
        .find_map(|(index, clip)| {
            let start_x = state.time_to_x(clip.start, width);
            let end_x = state.time_to_x(clip.end, width);
            clip_edge_at(x, start_x, end_x).map(|is_start| (index, is_start))
        })
}

pub fn zoom_clip_at(state: &VideoEditState, width: f64, x: f64) -> Option<usize> {
    state.zoom_clips.iter().position(|clip| {
        let start_x = state.time_to_x(clip.start, width);
        let end_x = state.time_to_x(clip.end, width);
        x >= start_x && x <= end_x
    })
}

pub fn cursor_hide_edge_at(state: &VideoEditState, width: f64, x: f64) -> Option<(usize, bool)> {
    state
        .cursor_hide_clips
        .iter()
        .enumerate()
        .find_map(|(index, clip)| {
            let start_x = state.time_to_x(clip.start, width);
            let end_x = state.time_to_x(clip.end, width);
            clip_edge_at(x, start_x, end_x).map(|is_start| (index, is_start))
        })
}

pub fn cursor_hide_clip_at(state: &VideoEditState, width: f64, x: f64) -> Option<usize> {
    state.cursor_hide_clips.iter().position(|clip| {
        let start_x = state.time_to_x(clip.start, width);
        let end_x = state.time_to_x(clip.end, width);
        x >= start_x && x <= end_x
    })
}

pub fn seek_to_x(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    width: f64,
    x: f64,
) {
    let seek_to = {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = x_to_timeline(&guard, width, x);
        guard.source_playhead()
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.seek((seek_to * 1_000_000.0) as i64);
    }
}

pub fn rightmost_video_segment(state: &VideoEditState, width: f64) -> Option<usize> {
    video_layout(state, width)
        .into_iter()
        .max_by(|left, right| left.3.total_cmp(&right.3))
        .map(|(_, segment, _, _)| segment)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipResizeAnchor {
    pub kind: ClipResizeKind,
    pub source_edge: f64,
    pub timeline_edge: f64,
    pub speed: f64,
    pub pixels_per_second: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipResizeKind {
    Start,
    End,
    Cut(usize),
}

impl ClipResizeAnchor {
    fn edges(
        state: &VideoEditState,
        segment: usize,
        drag: ClipDrag,
    ) -> Option<(ClipResizeKind, f64, f64, f64)> {
        let bounds = state.segment_boundaries();
        let &(source_start, source_end) = bounds.get(segment)?;
        let speed = state.segment_speed(segment);
        match drag {
            ClipDrag::Start => Some((
                ClipResizeKind::Start,
                source_start,
                state.segment_start(segment),
                speed,
            )),
            ClipDrag::End => {
                let hold = if state.freeze_applies_to_segment(segment) {
                    state.freeze_tail_seconds()
                } else {
                    0.0
                };
                Some((
                    ClipResizeKind::End,
                    source_end + hold * speed,
                    state.segment_start(segment) + state.segment_timeline_duration(segment) + hold,
                    speed,
                ))
            }
            ClipDrag::Cut(cut_index) => {
                let cut = *state.cuts.get(cut_index)?;
                Some((
                    ClipResizeKind::Cut(cut_index),
                    cut,
                    state.segment_start(segment) + (cut - source_start).max(0.0) / speed,
                    speed,
                ))
            }
            _ => None,
        }
    }

    pub fn for_video(
        state: &VideoEditState,
        width: f64,
        segment: usize,
        drag: ClipDrag,
    ) -> Option<ClipResizeAnchor> {
        let (kind, source_edge, timeline_edge, speed) = Self::edges(state, segment, drag)?;
        Some(ClipResizeAnchor {
            kind,
            source_edge,
            timeline_edge,
            speed,
            pixels_per_second: pixels_per_second(state, width),
        })
    }

    pub fn for_extend(state: &VideoEditState, width: f64) -> Option<ClipResizeAnchor> {
        let segment = rightmost_video_segment(state, width)?;
        let drag = segment_edge_drag(state, segment, false)?;
        ClipResizeAnchor::for_video(state, width, segment, drag)
    }
}

pub fn apply_clip_resize(
    state: &mut VideoEditState,
    width: f64,
    anchor: &ClipResizeAnchor,
    offset_x: f64,
) {
    let desired = anchor.timeline_edge + offset_x / anchor.pixels_per_second.max(1e-6);
    let snapped = snap_timeline_to_playhead(state, width, desired.max(0.0));
    let source = anchor.source_edge + (snapped - anchor.timeline_edge) * anchor.speed;
    match anchor.kind {
        ClipResizeKind::End => state.set_trim_end(source),
        ClipResizeKind::Start => {
            let old_source = state.trim_start_seconds;
            let old_comp = state.segment_start(0);
            state.set_trim_start(source);
            if !state.cuts.is_empty() && state.segments_kept.first().copied().unwrap_or(true) {
                let start = old_comp
                    + (state.trim_start_seconds - old_source) / state.segment_speed(0).max(1e-6);
                state.set_segment_start(0, start);
            }
        }
        ClipResizeKind::Cut(index) => {
            let Some(before_cut) = state.cuts.get(index).copied() else {
                return;
            };
            let right = index + 1;
            let before_comp = state.segment_start(right);
            state.move_cut(index, source);
            let Some(new_cut) = state.cuts.get(index).copied() else {
                return;
            };
            if state.segments_kept.get(right).copied().unwrap_or(true) {
                let start = before_comp
                    + (new_cut - before_cut) / state.segment_speed(right).max(1e-6);
                state.set_segment_start(right, start);
            }
        }
    }
}

#[cfg(test)]
pub fn edge_target_at(state: &VideoEditState, timeline_t: f64) -> f64 {
    let Some(segment) = rightmost_video_segment(state, 1000.0) else {
        return state.trim_end_seconds;
    };
    let Some((_, source_edge, timeline_edge, speed)) =
        ClipResizeAnchor::edges(state, segment, ClipDrag::End)
    else {
        return state.trim_end_seconds;
    };
    source_edge + (timeline_t - timeline_edge) * speed
}

pub fn x_to_timeline(state: &VideoEditState, width: f64, x: f64) -> f64 {
    state.x_to_time(x, width).max(0.0)
}

/// True when `x` is in the extend band: the mover at the clip's edge plus the
/// frosted affordance past it.
///
/// The band spans every lane, so a drag here extends the clip whichever row
/// the pointer is over — the user does not have to reach the main clip's own
/// ending edge on the video row.
pub fn in_extend_band(state: &VideoEditState, width: f64, x: f64) -> bool {
    let end_x = video_end_x(state, width);
    end_x > 0.0 && x >= end_x - EXTEND_HANDLE_HIT
}


pub fn pixels_per_second(state: &VideoEditState, width: f64) -> f64 {
    width.max(1.0) / state.visible_span_seconds().max(0.001)
}

/// Source-time spans for the filmstrip tiles painted across the video clip.
///
/// Tile `i` starts at the ffmpeg sample time the thumbnail was extracted at
/// and extends to the next tile's start (the last tile extends one full step
/// past its own start, capped at the usable end). Callers map the span ends
/// through `source_to_x`, so cuts, per-segment speed, trim and scroll all
/// apply without extra math here.
pub fn filmstrip_tile_times(duration: f64, count: usize) -> Vec<(f64, f64)> {
    use crate::recording::editor::ffmpeg::{thumbnail_count, thumbnail_timestamp};
    let count = if count == 0 {
        thumbnail_count(duration)
    } else {
        count
    };
    if count == 0 || !duration.is_finite() || duration <= 0.0 {
        return vec![(0.0, 0.0); count];
    }
    let step = duration / count as f64;
    (0..count)
        .map(|index| {
            let start = thumbnail_timestamp(duration, index, count).max(0.0);
            let end = if index + 1 < count {
                thumbnail_timestamp(duration, index + 1, count).max(start)
            } else {
                (start + step).min(duration).max(start)
            };
            (start, end)
        })
        .collect()
}

/// Pixel spans for each filmstrip tile in the current view.
pub fn filmstrip_tile_spans(state: &VideoEditState, width: f64) -> Vec<(f64, f64)> {
    let count =
        crate::recording::editor::ffmpeg::thumbnail_count(state.metadata.duration_seconds);
    filmstrip_tile_times(state.metadata.duration_seconds, count)
        .into_iter()
        .map(|(start, end)| {
            (
                state.source_to_x(start, width),
                state.source_to_x(end, width).max(state.source_to_x(start, width)),
            )
        })
        .collect()
}

pub fn playhead_snap_threshold(state: &VideoEditState, width: f64) -> f64 {
    PLAYHEAD_SNAP / pixels_per_second(state, width).max(1e-6)
}

pub fn snap_timeline_to_playhead(state: &VideoEditState, width: f64, time: f64) -> f64 {
    snap_to_target(
        time,
        state.playhead_seconds,
        playhead_snap_threshold(state, width),
    )
    .max(0.0)
}

pub fn snap_range_start_to_playhead(
    state: &VideoEditState,
    width: f64,
    start: f64,
    duration: f64,
) -> f64 {
    snap_range_to_target(
        start,
        duration,
        state.playhead_seconds,
        playhead_snap_threshold(state, width),
    )
}

pub fn sync_scroll_adj(adj: &Adjustment, state: &VideoEditState, syncing: &Cell<bool>) {
    let visible = state.visible_span_seconds();
    let upper = state.timeline_canvas_seconds().max(visible);
    let value = state.timeline_scroll_seconds;
    if (adj.page_size() - visible).abs() < 1e-6
        && (adj.upper() - upper).abs() < 1e-6
        && (adj.value() - value).abs() < 1e-4
    {
        return;
    }
    syncing.set(true);
    adj.set_lower(0.0);
    adj.set_page_size(visible);
    adj.set_upper(upper);
    adj.set_step_increment((visible * 0.05).max(0.05));
    adj.set_page_increment((visible * 0.8).max(0.1));
    if (adj.value() - value).abs() > 1e-4 {
        adj.set_value(value);
    }
    syncing.set(false);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipDrag {
    Start,
    End,
    Cut(usize),
    Move {
        origin_offset: f64,
        pixels_per_second: f64,
    },
    Segment {
        index: usize,
        origin_start: f64,
        pixels_per_second: f64,
    },
    Seek,
}

#[derive(Clone, Copy)]
pub enum ZoomDrag {
    Edge {
        index: usize,
        is_start: bool,
    },
    Move {
        index: usize,
        origin_start: f64,
        pixels_per_second: f64,
    },
    Seek,
    /// The pointer is in the extend band, so the drag resizes the clip's end
    /// even though it is over the zoom lane.
    Extend,
}

#[derive(Clone, Copy)]
pub enum HideDrag {
    Edge {
        index: usize,
        is_start: bool,
    },
    Move {
        index: usize,
        origin_start: f64,
        pixels_per_second: f64,
    },
    Seek,
    /// Same as `ZoomDrag::Extend`: the band spans every lane.
    Extend,
}

#[derive(Clone, Copy)]
pub enum TrackCursor {
    None,
    ResizeStart,
    ResizeEnd,
    Grab,
    Playhead,
}

pub const HANDLE_INSET: f64 = 6.0;
pub const HANDLE_WIDTH: f64 = 4.0;
pub const HANDLE_HIT: f64 = 6.0;
pub const PLAYHEAD_HIT: f64 = 6.0;
pub const PLAYHEAD_SNAP: f64 = 12.0;
/// Half the mover bar's width: the hit slop that makes the whole handle
/// grabbable even though it straddles the clip's right edge.
pub const EXTEND_HANDLE_HIT: f64 = 3.0;

#[cfg(test)]
mod tests {
    use super::filmstrip_tile_times;

    #[test]
    fn filmstrip_tiles_cover_duration_without_touching_eof() {
        let duration = 10.0;
        let tiles = filmstrip_tile_times(duration, 0);
        assert_eq!(tiles.len(), 12);
        assert!((tiles[0].0 - 0.0).abs() < 1e-9);
        for window in tiles.windows(2) {
            assert!(window[0].0 <= window[0].1);
            assert!(window[1].0 >= window[0].0);
            assert!((window[1].0 - window[0].1).abs() < 1e-9);
        }
        let last = tiles.last().unwrap();
        assert!(last.0 < last.1);
        assert!(last.0 < duration, "last tile must start from a pre-EOF sample");
        assert!(last.1 <= duration, "last tile must not run past EOF");
        assert!(last.1 > duration * 0.9, "last tile must still reach near the end");
    }

    #[test]
    fn filmstrip_tiles_degrade_on_degenerate_durations() {
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let tiles = filmstrip_tile_times(duration, 0);
            assert_eq!(tiles.len(), 12, "duration {duration}");
            assert!(
                tiles.iter().all(|&(start, end)| start == 0.0 && end == 0.0),
                "duration {duration}: {tiles:?}"
            );
        }
    }

    #[test]
    fn filmstrip_tile_count_overrides_default() {
        let tiles = filmstrip_tile_times(8.0, 4);
        assert_eq!(tiles.len(), 4);
        assert!((tiles[0].0 - 0.0).abs() < 1e-9);
        let last = tiles.last().unwrap();
        assert!(last.0 < 8.0);
        assert!(last.1 <= 8.0);
    }
}

#[cfg(test)]
mod freeze_edge_tests {
    use super::{
        edge_target_at, in_extend_band, video_end_x, video_hit, video_layout, ClipDrag,
        TrackCursor,
    };
    use crate::recording::editor::model::{VideoEditState, VideoMetadata};
    use std::path::PathBuf;

    fn state() -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 1024,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    /// The real bug: the right handle used to feed `set_trim_end` a value
    /// already clamped to the source end, so the first expansion was a no-op.
    #[test]
    fn dragging_past_the_source_end_opens_a_freeze_hold() {
        let mut state = state();
        let width = 1000.0;
        // The clip spans the full ruler at fit zoom; the handle sits at x1.
        let (_, _, x0, x1) = video_layout(&state, width)[0];

        // Grab the right edge and pull one second's worth of pixels right.
        let hit = video_hit(&state, width, x1 - 1.0);
        assert!(
            hit.drag == Some(ClipDrag::End),
            "the right edge must offer the End drag, got {:?}",
            hit.drag
        );

        // The pointer now sits past the last frame.
        let composition_t = state.x_to_time(x1 + 50.0, width);
        let target = edge_target_at(&state, composition_t);
        state.set_trim_end(target);

        assert!(
            state.freeze_tail_seconds() > 0.0,
            "dragging right past the end must open a hold, tail was {}",
            state.freeze_tail_seconds()
        );
        let _ = x0;
    }

    #[test]
    fn the_hold_keeps_the_right_handle_grabbable() {
        let mut state = state();
        let width = 1000.0;
        state.extend_last_segment(2.0);
        let (_, _, x0, x1) = video_layout(&state, width)[0];
        // The hit box must now extend to the end of the hold, so the handle
        // is still reachable out where the clip is actually drawn.
        assert!(x1 > 0.0 && x0 >= 0.0);
        let hit = video_hit(&state, width, x1 - 1.0);
        assert!(
            hit.drag == Some(ClipDrag::End),
            "the handle must remain grabbable at the held edge, got {:?}",
            hit.drag
        );
    }

    #[test]
    fn an_edge_inside_the_clip_still_trims_real_frames() {
        let mut state = state();
        let width = 1000.0;
        state.extend_last_segment(2.0);
        let (_, _, _, x1) = video_layout(&state, width)[0];
        // Pull the handle back inside the source: the hold gives way first,
        // and only then does real footage start disappearing.
        let inside = state.x_to_time(x1 - 300.0, width);
        let target = edge_target_at(&state, inside);
        state.set_trim_end(target);
        assert_eq!(state.freeze_tail_seconds(), 0.0, "hold is spent first");
        assert!(
            state.trim_end_seconds < 10.0,
            "then real frames trim, end was {}",
            state.trim_end_seconds
        );
    }

    /// The handle must track the pointer, not settle at half the drag.
    ///
    /// `edge_target_at` used to measure the overshoot from `last_segment_end()`,
    /// which already includes the hold being set, so each update computed
    /// `new = overshoot - old`: the tail converged to half the drag while
    /// oscillating frame to frame. Feeding the same absolute pointer in
    /// repeatedly has to be a fixed point, and it has to hold the full
    /// overshoot.
    #[test]
    fn the_drag_tracks_the_pointer_without_halving() {
        let mut state = state();
        let width = 1000.0;
        let (_, _, _, x1) = video_layout(&state, width)[0];

        // Three seconds of pixels past the source end.
        let composition_t = state.x_to_time(x1 + 300.0, width);
        let expected = composition_t - state.source_duration();

        let mut previous = -1.0;
        for _ in 0..5 {
            let target = edge_target_at(&state, composition_t);
            state.set_trim_end(target);
            let held = state.freeze_tail_seconds();
            assert!(
                (held - expected).abs() < 1e-6,
                "the tail must hold the full overshoot {expected}, got {held}",
            );
            assert!(
                (held - previous).abs() < 1e-6 || previous < 0.0,
                "repeating the same pointer must not oscillate: {previous} then {held}",
            );
            previous = held;
        }
    }

    /// Everything to the right of the clip is the extend handle, so the
    /// blurred band is draggable, not dead lane.
    #[test]
    fn the_trailing_band_offers_the_end_drag() {
        let mut state = state();
        let width = 1000.0;
        // Trim two seconds in, leaving a visible trailing band at fit zoom.
        state.set_trim_end(8.0);

        let hit = video_hit(&state, width, 900.0);
        assert_eq!(
            hit.drag,
            Some(ClipDrag::End),
            "the band past the clip must offer the End drag",
        );
        assert!(
            matches!(hit.cursor, TrackCursor::ResizeEnd),
            "the band must show the resize cursor",
        );
        assert_eq!(hit.segment, Some(0), "the band belongs to the last clip");
    }

    /// The mover straddles the clip's right edge, so its hit region has to
    /// start slightly before `end_x` — otherwise the left half of the handle
    /// is dead and the user has to find the exact pixel of the clip's edge.
    #[test]
    fn the_mover_is_grabbable_either_side_of_the_clip_edge() {
        let mut state = state();
        let width = 1000.0;
        state.set_trim_end(8.0);
        let end_x = video_end_x(&state, width);
        for x in [end_x - 2.0, end_x, end_x + 2.0] {
            let hit = video_hit(&state, width, x);
            assert_eq!(hit.drag, Some(ClipDrag::End), "x={x}");
            assert!(
                matches!(hit.cursor, TrackCursor::ResizeEnd),
                "x={x} must show the extend cursor"
            );
        }
    }

    /// The band and its mover are matched on every lane, so a drag over the
    /// zoom or hide rows extends the clip instead of depending on the video
    /// lane's own ending edge.
    #[test]
    fn the_extend_band_covers_the_mover_and_the_space_past_the_clip() {
        let mut state = state();
        let width = 1000.0;
        state.set_trim_end(8.0);
        let end_x = video_end_x(&state, width);
        assert!(!in_extend_band(&state, width, end_x - 10.0));
        assert!(in_extend_band(&state, width, end_x - 2.0), "left half of the mover");
        assert!(in_extend_band(&state, width, end_x + 50.0), "past the clip");
    }
}

#[cfg(test)]
mod resize_anchor_tests {
    use super::{
        apply_clip_resize, rightmost_video_segment, video_end_x, video_hit,
        ClipResizeAnchor, ClipResizeKind, ClipDrag,
    };
    use crate::recording::editor::model::{VideoEditState, VideoMetadata};
    use std::path::PathBuf;

    fn state13() -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 13.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 1024,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    fn joined_fixture() -> VideoEditState {
        let mut state = state13();
        state.add_cut(3.0);
        state.add_cut(6.0);
        state.selected_segment = Some(1);
        state.remove_selected_clip();
        state.set_segment_start(2, 3.0);
        state
    }

    #[test]
    fn end_resize_holds_its_anchor_and_restores_the_real_frames() {
        let mut state = joined_fixture();
        let width = 1300.0;
        let anchor =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::End).expect("end anchor");
        assert!((anchor.timeline_edge - 10.0).abs() < 1e-9);
        assert!((anchor.source_edge - 13.0).abs() < 1e-9);
        let pps = anchor.pixels_per_second;

        for _ in 0..5 {
            apply_clip_resize(&mut state, width, &anchor, -2.0 * pps);
            assert!(
                (state.trim_end_seconds - 11.0).abs() < 1e-9,
                "out {}",
                state.trim_end_seconds
            );
            assert_eq!(state.freeze_tail_seconds(), 0.0);
            assert!((state.video_end_seconds() - 8.0).abs() < 1e-9);
        }

        let restore =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::End).expect("restore anchor");
        apply_clip_resize(&mut state, width, &restore, 2.0 * pps);
        assert!((state.trim_end_seconds - 13.0).abs() < 1e-9);
        assert_eq!(state.freeze_tail_seconds(), 0.0);
        assert!((state.video_end_seconds() - 10.0).abs() < 1e-9);

        apply_clip_resize(&mut state, width, &restore, 3.0 * pps);
        assert!((state.trim_end_seconds - 13.0).abs() < 1e-9);
        assert!((state.freeze_tail_seconds() - 1.0).abs() < 1e-9);
        assert!((state.video_end_seconds() - 11.0).abs() < 1e-9);
    }

    #[test]
    fn a_cut_resize_keeps_the_right_clip_outpoint_and_repeats_stably() {
        let mut state = joined_fixture();
        let width = 1300.0;
        let anchor =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::Cut(1)).expect("cut anchor");
        assert_eq!(anchor.kind, ClipResizeKind::Cut(1));
        assert!((anchor.timeline_edge - 3.0).abs() < 1e-9);
        assert!((anchor.source_edge - 6.0).abs() < 1e-9);
        let pps = anchor.pixels_per_second;

        for _ in 0..5 {
            apply_clip_resize(&mut state, width, &anchor, 2.0 * pps);
            assert!((state.cuts[1] - 8.0).abs() < 1e-9, "cut {}", state.cuts[1]);
            assert!(
                (state.segment_start(2) - 5.0).abs() < 1e-9,
                "start {}",
                state.segment_start(2)
            );
            assert!((state.video_end_seconds() - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn an_extreme_shrink_keeps_the_floor_and_reespands_real_frames() {
        let mut state = joined_fixture();
        let width = 1300.0;
        let anchor =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::End).expect("end anchor");
        let pps = anchor.pixels_per_second;

        apply_clip_resize(&mut state, width, &anchor, -100.0 * pps);
        assert!(
            (state.trim_end_seconds - 6.25).abs() < 1e-9,
            "out {}",
            state.trim_end_seconds
        );
        assert_eq!(state.freeze_tail_seconds(), 0.0);

        apply_clip_resize(&mut state, width, &anchor, 0.0);
        assert!((state.trim_end_seconds - 13.0).abs() < 1e-9);
        assert_eq!(state.freeze_tail_seconds(), 0.0);
    }

    #[test]
    fn a_retimed_clip_converts_overshoot_through_its_speed() {
        let mut state = joined_fixture();
        let width = 1300.0;
        state.segment_speeds[2] = 2.0;
        let anchor =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::End).expect("end anchor");
        assert!((anchor.timeline_edge - 6.5).abs() < 1e-9);
        let pps = anchor.pixels_per_second;

        apply_clip_resize(&mut state, width, &anchor, 1.0 * pps);
        assert!((state.freeze_tail_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_mid_drag_scroll_does_not_move_the_target() {
        let mut state = joined_fixture();
        let width = 1300.0;
        let anchor =
            ClipResizeAnchor::for_video(&state, width, 2, ClipDrag::End).expect("end anchor");
        let pps = anchor.pixels_per_second;

        apply_clip_resize(&mut state, width, &anchor, -2.0 * pps);
        let target = state.trim_end_seconds;
        state.set_timeline_scroll(5.0);
        apply_clip_resize(&mut state, width, &anchor, -2.0 * pps);
        assert!((state.trim_end_seconds - target).abs() < 1e-9);
    }

    #[test]
    fn a_deleted_original_tail_leaves_the_shared_band_on_the_retained_cut() {
        let mut state = state13();
        state.add_cut(6.0);
        state.selected_segment = Some(1);
        state.remove_selected_clip();
        let width = 1300.0;
        let end_x = video_end_x(&state, width);
        assert!(rightmost_video_segment(&state, width) == Some(0));
        let hit = video_hit(&state, width, end_x + 1.0);
        assert_eq!(hit.drag, Some(ClipDrag::Cut(0)));
        let anchor = ClipResizeAnchor::for_extend(&state, width).expect("band anchor");
        assert_eq!(anchor.kind, ClipResizeKind::Cut(0));
    }
}
