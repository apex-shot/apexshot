//! X11 pointer tracking without a shell extension.
//!
//! Polls the root pointer and the XFixes cursor name so X11 sessions get the
//! same pointer sidecar the shell extension provides on GNOME. The poll loop
//! runs on its own thread with its own X connection; the state machine is a
//! plain struct driven by `(t, x, y, mask, kind)` samples.

use crate::gnome_shell::PointerTrackResult;
use gstreamer::glib;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::ConnectionExt as XFixesExt;
use x11rb::protocol::xproto::ConnectionExt as XprotoExt;

use super::editor::sidecar::{CaptureRegion, MAX_CLICKS, MAX_POINTER_SAMPLES, MAX_PRESSES};

/// Pointer and button poll interval.
const POINTER_POLL: Duration = Duration::from_millis(16);
/// A still pointer is sampled at least this often so the editor can see dwells.
const STILL_SAMPLE_INTERVAL_SECONDS: f64 = 0.1;
/// Duplicate clicks closer than this in time and space are the same press.
const CLICK_DEDUPE_SECONDS: f64 = 0.03;
const CLICK_DEDUPE_PX: i32 = 2;
/// How far a held pointer must move to count as a drag.
const DRAG_THRESHOLD_PX: f64 = 4.0;
/// XFixes cursor names are re-read at this cadence.
const CURSOR_NAME_INTERVAL_US: i64 = 100_000;

const BUTTON1_MASK: u16 = 1 << 8;
const BUTTON2_MASK: u16 = 1 << 9;
const BUTTON3_MASK: u16 = 1 << 10;
const BUTTON_MASK: u16 = BUTTON1_MASK | BUTTON2_MASK | BUTTON3_MASK;

/// Map an X cursor name to the kind strings the editor understands.
///
/// Mirrors the shell extension's shape table so both trackers produce the
/// same cursor kinds for the same theme names.
fn cursor_kind_for_name(name: &str) -> &'static str {
    let name = name
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_ascii_lowercase();
    match name.as_str() {
        "crosshair" | "cross" | "cross_reverse" | "tcross" | "diamond_cross" | "dotbox"
        | "dot_box_mask" | "draped_box" | "cell" | "plus" | "color-picker" => "crosshair",
        "fleur" | "move" | "all-scroll" | "size_all" | "closedhand" | "grabbing" | "dnd-move" => {
            "move"
        }
        "text" | "xterm" | "ibeam" | "vertical-text" => "text",
        "left_ptr" | "default" | "arrow" | "top_left_arrow" | "left-arrow" => "default",
        "ew-resize" | "col-resize" | "e-resize" | "w-resize" | "sb_h_double_arrow"
        | "h_double_arrow" | "left_side" | "right_side" | "size_hor" | "split_h" => "resize_h",
        "ns-resize" | "row-resize" | "n-resize" | "s-resize" | "sb_v_double_arrow"
        | "v_double_arrow" | "double_arrow" | "top_side" | "bottom_side" | "size_ver"
        | "split_v" => "resize_v",
        "nwse-resize" | "nesw-resize" | "bd_double_arrow" | "fd_double_arrow" | "nw-resize"
        | "ne-resize" | "sw-resize" | "se-resize" | "size_fdiag" | "size_bdiag" => "resize_diag",
        "wait" | "watch" | "progress" | "half-busy" | "left_ptr_watch" => "wait",
        "pointer" | "hand2" | "hand" | "pointing_hand" | "hand1" | "openhand" | "grab" => "hand",
        _ => "default",
    }
}

/// Keep the first item, every other one, and the last.
///
/// Same compaction the shell extension applies when a sample cap is reached.
fn compact_keep_alternating<T>(items: &mut Vec<T>) {
    let last = items.len().saturating_sub(1);
    if last == 0 {
        return;
    }
    let original = std::mem::take(items);
    let mut compacted = Vec::with_capacity(last / 2 + 2);
    for (index, item) in original.into_iter().enumerate() {
        if index == 0 || index == last || (index % 2 == 0 && index < last) {
            compacted.push(item);
        }
    }
    *items = compacted;
}

#[derive(Debug, Clone, Copy)]
struct OpenPress {
    button: i32,
    down_t: f64,
    x: i32,
    y: i32,
    dragged: bool,
}

#[derive(Debug, Default)]
struct PressTracker {
    open: Vec<OpenPress>,
    presses: Vec<(f64, f64, i32, bool)>,
}

impl PressTracker {
    fn press(&mut self, button: i32, t: f64, x: i32, y: i32) {
        if self.open.iter().any(|press| press.button == button) {
            return;
        }
        self.open.push(OpenPress {
            button,
            down_t: t,
            x,
            y,
            dragged: false,
        });
    }

    fn move_to(&mut self, x: i32, y: i32) {
        for press in &mut self.open {
            let dx = f64::from(x - press.x);
            let dy = f64::from(y - press.y);
            if dx.hypot(dy) > DRAG_THRESHOLD_PX {
                press.dragged = true;
            }
        }
    }

    fn release(&mut self, button: i32, t: f64) {
        let Some(index) = self.open.iter().position(|press| press.button == button) else {
            return;
        };
        let press = self.open.remove(index);
        self.presses
            .push((press.down_t, t, press.button, press.dragged));
        if self.presses.len() > MAX_PRESSES {
            compact_keep_alternating(&mut self.presses);
        }
    }

    fn close_all(&mut self, t: f64) {
        let buttons: Vec<i32> = self.open.iter().map(|press| press.button).collect();
        for button in buttons {
            self.release(button, t);
        }
    }

    fn into_presses(self) -> Vec<(f64, f64, i32, bool)> {
        self.presses
    }
}

/// The recording-time pointer state machine.
struct PointerTracker {
    samples: Vec<(f64, i32, i32, String)>,
    clicks: Vec<(f64, i32, i32, i32)>,
    presses: PressTracker,
    button_mask: u16,
}

impl PointerTracker {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            clicks: Vec::new(),
            presses: PressTracker::default(),
            button_mask: 0,
        }
    }

    /// Record the first sample without treating already-held buttons as clicks.
    fn begin(&mut self, t: f64, x: i32, y: i32, mask: u16, kind: &str) {
        self.button_mask = mask & BUTTON_MASK;
        self.push_sample(t, x, y, kind, true);
    }

    fn poll(&mut self, t: f64, x: i32, y: i32, mask: u16, kind: &str) {
        self.presses.move_to(x, y);
        self.sample_buttons(t, x, y, mask);
        self.push_sample(t, x, y, kind, false);
    }

    fn sample_buttons(&mut self, t: f64, x: i32, y: i32, mask: u16) {
        let current = mask & BUTTON_MASK;
        let pressed = current & !self.button_mask;
        let released = self.button_mask & !current;
        for (button, bit) in [(1, BUTTON1_MASK), (2, BUTTON2_MASK), (3, BUTTON3_MASK)] {
            if pressed & bit != 0 {
                self.record_click(t, x, y, button);
                self.presses.press(button, t, x, y);
            }
            if released & bit != 0 {
                self.presses.release(button, t);
            }
        }
        self.button_mask = current;
    }

    fn record_click(&mut self, t: f64, x: i32, y: i32, button: i32) {
        if let Some(&(last_t, last_x, last_y, last_button)) = self.clicks.last() {
            if last_button == button
                && (t - last_t).abs() < CLICK_DEDUPE_SECONDS
                && (x - last_x).abs() <= CLICK_DEDUPE_PX
                && (y - last_y).abs() <= CLICK_DEDUPE_PX
            {
                return;
            }
        }
        self.clicks.push((t, x, y, button));
        if self.clicks.len() > MAX_CLICKS {
            compact_keep_alternating(&mut self.clicks);
        }
    }

    fn push_sample(&mut self, t: f64, x: i32, y: i32, kind: &str, force: bool) {
        if !force {
            if let Some(last) = self.samples.last() {
                let still = last.1 == x && last.2 == y && last.3 == kind;
                if still && (t - last.0) < STILL_SAMPLE_INTERVAL_SECONDS {
                    return;
                }
            }
        }
        self.samples.push((t, x, y, kind.to_string()));
        if self.samples.len() >= MAX_POINTER_SAMPLES {
            compact_keep_alternating(&mut self.samples);
        }
    }

    /// Close any still-held press at the stop instant and hand back the data.
    fn finish(mut self, stop_t: f64) -> PointerTrackResult {
        self.presses.close_all(stop_t);
        PointerTrackResult {
            t0_monotonic_us: 0,
            samples: self.samples,
            clicks: self.clicks,
            presses: self.presses.into_presses(),
        }
    }
}

struct TrackerHandle {
    stop_tx: mpsc::Sender<()>,
    join: std::thread::JoinHandle<PointerTrackResult>,
}

static ACTIVE: Mutex<Option<TrackerHandle>> = Mutex::new(None);

fn empty_result() -> PointerTrackResult {
    PointerTrackResult {
        t0_monotonic_us: 0,
        samples: Vec::new(),
        clicks: Vec::new(),
        presses: Vec::new(),
    }
}

pub(super) fn is_active() -> bool {
    ACTIVE.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

/// The whole X screen as a capture region, for fullscreen recordings whose
/// config carries no explicit area.
pub(super) fn screen_region() -> Option<CaptureRegion> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    Some(CaptureRegion {
        x: 0,
        y: 0,
        w: u32::from(screen.width_in_pixels),
        h: u32::from(screen.height_in_pixels),
    })
}

fn stop_active() {
    if let Some(handle) = ACTIVE.lock().ok().and_then(|mut guard| guard.take()) {
        let _ = handle.stop_tx.send(());
        let _ = handle.join.join();
    }
}

pub(super) fn start() -> anyhow::Result<()> {
    stop_active();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name("apexshot-x11-pointer".into())
        .spawn(move || track(stop_rx, ready_tx))
        .map_err(|err| anyhow::anyhow!("failed to spawn X11 pointer tracker: {err}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {
            if let Ok(mut guard) = ACTIVE.lock() {
                *guard = Some(TrackerHandle { stop_tx, join });
            }
            Ok(())
        }
        Ok(Err(err)) => {
            let _ = join.join();
            Err(err)
        }
        Err(_) => {
            let _ = stop_tx.send(());
            Err(anyhow::anyhow!("X11 pointer tracker did not start"))
        }
    }
}

pub(super) fn stop() -> anyhow::Result<PointerTrackResult> {
    let Some(handle) = ACTIVE.lock().ok().and_then(|mut guard| guard.take()) else {
        return Ok(empty_result());
    };
    let _ = handle.stop_tx.send(());
    Ok(handle.join.join().unwrap_or_else(|_| empty_result()))
}

fn elapsed_seconds(t0_us: i64, now_us: i64) -> f64 {
    now_us.saturating_sub(t0_us) as f64 / 1_000_000.0
}

fn query_pointer(
    conn: &x11rb::rust_connection::RustConnection,
    root: x11rb::protocol::xproto::Window,
) -> Option<(i32, i32, u16)> {
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((
        i32::from(reply.root_x),
        i32::from(reply.root_y),
        u16::from(reply.mask) & BUTTON_MASK,
    ))
}

fn query_cursor_kind(conn: &x11rb::rust_connection::RustConnection) -> String {
    let reply = conn
        .xfixes_get_cursor_image_and_name()
        .ok()
        .and_then(|cookie| cookie.reply().ok());
    match reply {
        Some(reply) => cursor_kind_for_name(&String::from_utf8_lossy(&reply.name)).to_string(),
        None => "default".to_string(),
    }
}

fn track(
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<anyhow::Result<()>>,
) -> PointerTrackResult {
    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(connection) => connection,
        Err(err) => {
            let _ = ready_tx.send(Err(anyhow::anyhow!("failed to connect to X11: {err}")));
            return empty_result();
        }
    };
    let Some(root) = conn.setup().roots.get(screen_num).map(|screen| screen.root) else {
        let _ = ready_tx.send(Err(anyhow::anyhow!("X11 screen {} is missing", screen_num)));
        return empty_result();
    };
    let has_xfixes = match conn.xfixes_query_version(4, 0) {
        Ok(cookie) => cookie.reply().is_ok(),
        Err(_) => false,
    };
    let t0_us = glib::monotonic_time();
    let mut tracker = PointerTracker::new();
    let Some((x, y, mask)) = query_pointer(&conn, root) else {
        let _ = ready_tx.send(Err(anyhow::anyhow!("X11 QueryPointer failed")));
        return empty_result();
    };
    let mut kind = if has_xfixes {
        query_cursor_kind(&conn)
    } else {
        "default".to_string()
    };
    tracker.begin(
        elapsed_seconds(t0_us, glib::monotonic_time()),
        x,
        y,
        mask,
        &kind,
    );
    let _ = ready_tx.send(Ok(()));

    let mut last_cursor_query_us = glib::monotonic_time();
    loop {
        match stop_rx.recv_timeout(POINTER_POLL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now_us = glib::monotonic_time();
        let Some((x, y, mask)) = query_pointer(&conn, root) else {
            continue;
        };
        if has_xfixes && now_us - last_cursor_query_us >= CURSOR_NAME_INTERVAL_US {
            kind = query_cursor_kind(&conn);
            last_cursor_query_us = now_us;
        }
        tracker.poll(elapsed_seconds(t0_us, now_us), x, y, mask, &kind);
    }

    let now_us = glib::monotonic_time();
    if let Some((x, y, mask)) = query_pointer(&conn, root) {
        if has_xfixes {
            kind = query_cursor_kind(&conn);
        }
        tracker.poll(elapsed_seconds(t0_us, now_us), x, y, mask, &kind);
    }
    let mut result = tracker.finish(elapsed_seconds(t0_us, now_us));
    result.t0_monotonic_us = t0_us;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::CursorKind;

    fn started_tracker() -> PointerTracker {
        let mut tracker = PointerTracker::new();
        tracker.begin(0.0, 10, 10, 0, "default");
        tracker
    }

    #[test]
    fn button_press_records_a_click_and_its_press_interval() {
        let mut tracker = started_tracker();
        tracker.poll(0.5, 10, 10, BUTTON1_MASK, "default");
        tracker.poll(0.7, 10, 10, 0, "default");

        let result = tracker.finish(0.8);

        assert_eq!(result.clicks, vec![(0.5, 10, 10, 1)]);
        assert_eq!(result.presses, vec![(0.5, 0.7, 1, false)]);
    }

    #[test]
    fn a_press_becomes_a_drag_only_after_four_pixels() {
        let mut three = started_tracker();
        three.poll(0.2, 10, 10, BUTTON1_MASK, "default");
        three.poll(0.3, 13, 10, BUTTON1_MASK, "default");
        three.poll(0.4, 13, 10, 0, "default");
        assert_eq!(three.finish(0.5).presses, vec![(0.2, 0.4, 1, false)]);

        let mut five = started_tracker();
        five.poll(0.2, 10, 10, BUTTON1_MASK, "default");
        five.poll(0.3, 15, 10, BUTTON1_MASK, "default");
        five.poll(0.4, 15, 10, 0, "default");
        assert_eq!(five.finish(0.5).presses, vec![(0.2, 0.4, 1, true)]);
    }

    #[test]
    fn a_duplicate_press_in_the_same_spot_is_one_click() {
        let mut tracker = started_tracker();
        tracker.poll(1.000, 10, 10, BUTTON1_MASK, "default");
        tracker.poll(1.010, 10, 10, 0, "default");
        tracker.poll(1.020, 11, 11, BUTTON1_MASK, "default");

        let result = tracker.finish(1.2);

        assert_eq!(result.clicks, vec![(1.000, 10, 10, 1)]);
        // Both presses are kept even though the second click was deduplicated.
        assert_eq!(result.presses.len(), 2);
    }

    #[test]
    fn a_still_pointer_is_sampled_every_hundred_milliseconds() {
        let mut tracker = started_tracker();
        tracker.poll(0.05, 10, 10, 0, "default");
        tracker.poll(0.11, 10, 10, 0, "default");
        tracker.poll(0.16, 10, 10, 0, "default");
        tracker.poll(0.22, 10, 10, 0, "default");

        let result = tracker.finish(0.3);

        let times: Vec<f64> = result.samples.iter().map(|sample| sample.0).collect();
        assert_eq!(times, vec![0.0, 0.11, 0.22]);
    }

    #[test]
    fn compaction_keeps_the_first_every_other_and_the_last() {
        let mut items: Vec<u32> = (0..=5).collect();
        compact_keep_alternating(&mut items);
        assert_eq!(items, vec![0, 2, 4, 5]);

        let mut two = vec![0, 1];
        compact_keep_alternating(&mut two);
        assert_eq!(two, vec![0, 1]);

        let mut one = vec![0];
        compact_keep_alternating(&mut one);
        assert_eq!(one, vec![0]);
    }

    #[test]
    fn cursor_names_map_to_the_editor_kinds() {
        let cases = [
            ("left_ptr", "default"),
            ("default", "default"),
            ("arrow", "default"),
            ("hand1", "hand"),
            ("hand2", "hand"),
            ("pointer", "hand"),
            ("xterm", "text"),
            ("text", "text"),
            ("ibeam", "text"),
            ("crosshair", "crosshair"),
            ("cross", "crosshair"),
            ("watch", "wait"),
            ("left_ptr_watch", "wait"),
            ("fleur", "move"),
            ("grabbing", "move"),
            ("ew-resize", "resize_h"),
            ("sb_h_double_arrow", "resize_h"),
            ("ns-resize", "resize_v"),
            ("sb_v_double_arrow", "resize_v"),
            ("nwse-resize", "resize_diag"),
            ("fd_double_arrow", "resize_diag"),
            ("", "default"),
            ("mystery-name", "default"),
        ];
        for (name, expected) in cases {
            let kind = cursor_kind_for_name(name);
            assert_eq!(kind, expected, "name {name:?}");
            assert_eq!(CursorKind::parse(kind).as_str(), expected, "name {name:?}");
        }
    }

    #[test]
    fn presses_still_held_at_stop_close_at_the_stop_instant() {
        let mut tracker = started_tracker();
        tracker.poll(0.2, 10, 10, BUTTON1_MASK, "default");
        tracker.poll(0.3, 20, 20, BUTTON1_MASK | BUTTON2_MASK, "default");

        let result = tracker.finish(0.9);

        assert_eq!(
            result.presses,
            vec![(0.2, 0.9, 1, true), (0.3, 0.9, 2, false)]
        );
    }

    #[test]
    fn the_first_and_last_sample_are_always_kept() {
        let mut tracker = started_tracker();
        // Same position and kind for longer than the dwell interval: the
        // forced begin sample is the first, the forced final poll is the last.
        tracker.poll(0.05, 10, 10, 0, "default");
        tracker.poll(0.2, 10, 10, 0, "default");

        let result = tracker.finish(0.25);

        assert_eq!(result.samples.first().map(|sample| sample.0), Some(0.0));
        assert_eq!(result.samples.last().map(|sample| sample.0), Some(0.2));
    }
}
