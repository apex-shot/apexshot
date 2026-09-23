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
        let x1 = state.time_to_x(comp + state.segment_timeline_duration(seg_idx), width);
        layout.push((order_pos, seg_idx, x0, x1.max(x0 + 8.0)));
    }
    layout
}

pub struct VideoHit {
    pub cursor: TrackCursor,
    pub segment: Option<usize>,
    pub drag: Option<ClipDrag>,
}

pub fn video_hit(state: &VideoEditState, width: f64, x: f64) -> VideoHit {
    for &(_, seg_idx, x0, x1) in &video_layout(state, width) {
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
            None if state.cuts.is_empty() => (
                TrackCursor::Grab,
                Some(ClipDrag::Move {
                    origin_offset: state.timeline_offset_seconds,
                    pixels_per_second: pixels_per_second(state, width),
                }),
            ),
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

pub fn x_to_source(state: &VideoEditState, width: f64, x: f64) -> f64 {
    state
        .timeline_to_source(x_to_timeline(state, width, x))
        .clamp(0.0, state.metadata.duration_seconds.max(0.0))
}

pub fn x_to_timeline(state: &VideoEditState, width: f64, x: f64) -> f64 {
    state.x_to_time(x.clamp(0.0, width), width).max(0.0)
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

pub fn snap_source_to_playhead(state: &VideoEditState, width: f64, source_t: f64) -> f64 {
    snap_to_target(
        source_t,
        state.source_playhead(),
        playhead_snap_threshold(state, width),
    )
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

#[derive(Clone, Copy)]
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
