//! Record mouse clicks on KDE Plasma Wayland through a temporary KWin effect.
//!
//! Wayland gives an application no global button stream. A compositor script
//! effect can observe pointer buttons, but it has no D-Bus client, so its only
//! output channel is the journal. This module installs such an effect, reads the
//! journal beside it, and turns the button transitions back into click
//! intervals on the monotonic clock the recording timeline uses.
//!
//! Script effects only load while the compositor has hardware-accelerated
//! compositing. Every failure is silent to the caller: the editor already
//! places automatic zooms from where the cursor settles when a recording has
//! no click data.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::editor::sidecar::{ClickSample, PressSample};
use super::timeline::RecordingTimeline;

const EFFECT_ID: &str = "apexshot_click_bridge";
const METADATA_JSON: &str = r#"{"KPackageStructure":"KWin/Effect","KPlugin":{"Id":"apexshot_click_bridge","Name":"ApexShot click bridge","Version":"1.0"},"X-Plasma-API":"javascript","X-Plasma-MainScript":"code/main.js"}"#;
const CLICK_PREFIX: &str = "APEXSHOT_CLICK";
const CLICK_SAMPLE_TOLERANCE_US: i64 = 50_000;
const DRAG_THRESHOLD_SQUARED: i64 = 16;
const JOURNAL_START_SETTLE: Duration = Duration::from_millis(200);
const JOURNAL_DRAIN: Duration = Duration::from_millis(150);
/// Qt button flags: 1 left, 2 right, 4 middle.
const QT_BUTTON_BITS: [i32; 3] = [1, 2, 4];
/// The button numbers the sidecar stores: 1 left, 2 middle, 3 right.
const STORED_BUTTONS: [i32; 3] = [1, 3, 2];

/// One mouse button held from `down_us` to `up_us` on the monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RawClick {
    pub(super) down_us: i64,
    /// `None` when the button was still held when the recording stopped.
    pub(super) up_us: Option<i64>,
    pub(super) button: i32,
    pub(super) dragged: bool,
}

/// The effect id is fixed, so only one recording may own it at a time.
static ACTIVE: AtomicBool = AtomicBool::new(false);

struct SessionClaim;

impl Drop for SessionClaim {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

fn claim_session() -> Option<SessionClaim> {
    ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| SessionClaim)
}

/// A running click bridge: the installed effect, the journal reader, and the
/// package directory they share.
pub(super) struct KwinClickBridge {
    effect_dir: PathBuf,
    child: Option<Child>,
    reader: Option<std::thread::JoinHandle<Vec<RawClick>>>,
    claim: Option<SessionClaim>,
    stopped: bool,
}

impl KwinClickBridge {
    /// Install and load the effect. Returns `None` off KDE Wayland, when the
    /// journal cannot be read, or when the compositor refuses the effect.
    pub(super) fn start() -> Option<Self> {
        if !crate::backend::kde_screenshot::is_kde_wayland_session() {
            return None;
        }
        let claim = claim_session()?;
        match Self::try_start(claim) {
            Ok(bridge) => {
                eprintln!("[recording] KWin click bridge started");
                Some(bridge)
            }
            Err(err) => {
                eprintln!(
                    "[recording] KWin click bridge unavailable ({err}); Auto Zoom will use cursor settle points"
                );
                None
            }
        }
    }

    fn try_start(claim: SessionClaim) -> anyhow::Result<Self> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("the user data directory is unknown"))?;
        let effect_dir = data_dir.join("kwin/effects").join(EFFECT_ID);

        // A crashed previous run may have left the effect loaded and its
        // package on disk. Unload first so stale code cannot keep logging.
        let _ = set_effect_loaded(false);
        let _ = std::fs::remove_dir_all(&effect_dir);

        let nonce = session_nonce();
        write_effect_package(&effect_dir, &nonce)?;

        let mut child = spawn_journal_reader()?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("journalctl did not expose its output");
            }
        };
        let reader = spawn_reader(stdout, nonce);
        let mut bridge = Self {
            effect_dir,
            child: Some(child),
            reader: Some(reader),
            claim: Some(claim),
            stopped: false,
        };

        // The reader has to be attached before the effect can emit anything.
        std::thread::sleep(JOURNAL_START_SETTLE);
        let exited = bridge
            .child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten();
        if let Some(status) = exited {
            let _ = bridge.teardown();
            anyhow::bail!("journalctl exited before the effect loaded ({status})");
        }

        match set_effect_loaded(true) {
            Ok(true) => Ok(bridge),
            Ok(false) => {
                let _ = bridge.teardown();
                anyhow::bail!("the compositor refused the script effect (software rendering?)");
            }
            Err(err) => {
                let _ = bridge.teardown();
                Err(err)
            }
        }
    }

    /// Unload the effect, drain the last journal lines, and return the presses.
    pub(super) fn stop(mut self) -> Vec<RawClick> {
        self.teardown()
    }

    fn teardown(&mut self) -> Vec<RawClick> {
        if self.stopped {
            return Vec::new();
        }
        self.stopped = true;
        let _ = set_effect_loaded(false);
        std::thread::sleep(JOURNAL_DRAIN);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let presses = self
            .reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        let _ = std::fs::remove_dir_all(&self.effect_dir);
        self.claim.take();
        presses
    }
}

impl Drop for KwinClickBridge {
    fn drop(&mut self) {
        let _ = self.teardown();
    }
}

fn write_effect_package(effect_dir: &Path, nonce: &str) -> anyhow::Result<()> {
    let code_dir = effect_dir.join("contents/code");
    std::fs::create_dir_all(&code_dir)?;
    std::fs::write(effect_dir.join("metadata.json"), METADATA_JSON)?;
    std::fs::write(code_dir.join("main.js"), effect_script(nonce))?;
    Ok(())
}

fn effect_script(nonce: &str) -> String {
    format!(
        r#"function apexshotClickBridge(pos, oldPos, buttons, oldButtons) {{
    if (buttons === oldButtons) {{
        return;
    }}
    console.warn("{CLICK_PREFIX} {nonce} " + buttons + " " + oldButtons + " " +
        Math.round(pos.x) + " " + Math.round(pos.y));
}}

effects.mouseChanged.connect(apexshotClickBridge);
"#
    )
}

fn set_effect_loaded(loaded: bool) -> anyhow::Result<bool> {
    let method = if loaded { "loadEffect" } else { "unloadEffect" };
    let connection = zbus::blocking::Connection::session()
        .map_err(|err| anyhow::anyhow!("the session bus is unavailable: {err}"))?;
    let reply = connection
        .call_method(
            Some("org.kde.KWin"),
            "/Effects",
            Some("org.kde.kwin.Effects"),
            method,
            &EFFECT_ID,
        )
        .map_err(|err| anyhow::anyhow!("{method} failed: {err}"))?;
    reply
        .body()
        .deserialize::<bool>()
        .map_err(|err| anyhow::anyhow!("{method} returned a non-boolean result: {err}"))
}

fn spawn_journal_reader() -> anyhow::Result<Child> {
    std::process::Command::new("journalctl")
        .args([
            "--user",
            "--follow",
            "--lines=0",
            "--output=json",
            "--no-pager",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow::anyhow!("journalctl is unavailable: {err}"))
}

fn spawn_reader(stdout: ChildStdout, nonce: String) -> std::thread::JoinHandle<Vec<RawClick>> {
    std::thread::spawn(move || {
        let mut events = Vec::new();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(event) = parse_journal_line(&line, &nonce) {
                events.push(event);
            }
        }
        presses_from_events(&events)
    })
}

fn session_nonce() -> String {
    use std::io::Read;

    let mut bytes = [0u8; 8];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok()
    {
        return bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{:016x}", (nanos as u64) ^ u64::from(std::process::id()))
}

/// One button change as the effect logged it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KwinClickEvent {
    at_us: i64,
    buttons: i32,
    old_buttons: i32,
    x: i32,
    y: i32,
}

/// Parse one journal JSON line. Lines from another app, another session, or
/// another bridge instance are rejected by the nonce.
fn parse_journal_line(line: &str, nonce: &str) -> Option<KwinClickEvent> {
    // Most user-journal lines are unrelated; skip the JSON parse for them.
    if !line.contains(CLICK_PREFIX) {
        return None;
    }
    let entry: serde_json::Value = serde_json::from_str(line).ok()?;
    let message = journal_message(&entry)?;
    let at_us = journal_timestamp_us(&entry)?;
    parse_click_message(&message, nonce).map(|(buttons, old_buttons, x, y)| KwinClickEvent {
        at_us,
        buttons,
        old_buttons,
        x,
        y,
    })
}

fn journal_message(entry: &serde_json::Value) -> Option<String> {
    match entry.get("MESSAGE")? {
        serde_json::Value::String(message) => Some(message.clone()),
        serde_json::Value::Array(parts) => {
            let bytes: Option<Vec<u8>> = parts
                .iter()
                .map(|part| part.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                .collect();
            String::from_utf8(bytes?).ok()
        }
        _ => None,
    }
}

fn journal_timestamp_us(entry: &serde_json::Value) -> Option<i64> {
    match entry.get("__MONOTONIC_TIMESTAMP")? {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

fn parse_click_message(message: &str, nonce: &str) -> Option<(i32, i32, i32, i32)> {
    let rest = message.strip_prefix(CLICK_PREFIX)?.strip_prefix(' ')?;
    let rest = rest.strip_prefix(nonce)?.strip_prefix(' ')?;
    let mut parts = rest.split_whitespace();
    let buttons = parts.next()?.parse::<i32>().ok()?;
    let old_buttons = parts.next()?.parse::<i32>().ok()?;
    let x = parts.next()?.parse::<i32>().ok()?;
    let y = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((buttons, old_buttons, x, y))
}

/// Turn button-state transitions into press intervals. A bit going 0→1 opens a
/// press; the matching 1→0 closes it. A press still open at the end has no
/// release. Each button has its own state, so overlapping buttons stay apart.
fn presses_from_events(events: &[KwinClickEvent]) -> Vec<RawClick> {
    #[derive(Clone, Copy)]
    struct Down {
        at_us: i64,
        x: i32,
        y: i32,
    }

    let mut open: [Option<Down>; 3] = [None; 3];
    let mut presses = Vec::new();
    for event in events {
        let changed = event.buttons ^ event.old_buttons;
        for (index, &bit) in QT_BUTTON_BITS.iter().enumerate() {
            if changed & bit == 0 {
                continue;
            }
            if event.buttons & bit != 0 {
                if let Some(previous) = open[index].take() {
                    presses.push(RawClick {
                        down_us: previous.at_us,
                        up_us: None,
                        button: STORED_BUTTONS[index],
                        dragged: false,
                    });
                }
                open[index] = Some(Down {
                    at_us: event.at_us,
                    x: event.x,
                    y: event.y,
                });
            } else if let Some(down) = open[index].take() {
                let dx = i64::from(event.x) - i64::from(down.x);
                let dy = i64::from(event.y) - i64::from(down.y);
                presses.push(RawClick {
                    down_us: down.at_us,
                    up_us: Some(event.at_us.max(down.at_us)),
                    button: STORED_BUTTONS[index],
                    dragged: dx * dx + dy * dy > DRAG_THRESHOLD_SQUARED,
                });
            }
        }
    }
    for (index, down) in open.into_iter().enumerate() {
        if let Some(down) = down {
            presses.push(RawClick {
                down_us: down.at_us,
                up_us: None,
                button: STORED_BUTTONS[index],
                dragged: false,
            });
        }
    }
    presses.sort_unstable_by_key(|press| press.down_us);
    presses
}

/// Map recorded presses onto the stream cursor track. A click takes the cursor
/// position sampled within 50 ms of its press, or else the last position
/// before it. A click while the cursor was outside the recorded frame, area,
/// or window has no visible position, so it is dropped.
pub(super) fn clicks_and_presses(
    raw: &[RawClick],
    timeline: &RecordingTimeline,
) -> (Vec<ClickSample>, Vec<PressSample>) {
    let Some(origin_us) = timeline.origin_us() else {
        return (Vec::new(), Vec::new());
    };
    let mut clicks = Vec::new();
    let mut presses = Vec::new();
    for raw_click in raw {
        let Some((x, y)) = nearby_stream_position(timeline, raw_click.down_us) else {
            continue;
        };
        let down = (raw_click.down_us - origin_us) as f64 / 1_000_000.0;
        clicks.push(ClickSample {
            t: down,
            x,
            y,
            button: raw_click.button,
            cursor_id: None,
        });
        if let Some(up_us) = raw_click.up_us {
            let up = (up_us - origin_us) as f64 / 1_000_000.0;
            if up > down {
                presses.push(PressSample {
                    down,
                    up,
                    button: raw_click.button,
                    dragged: raw_click.dragged,
                });
            }
        }
    }
    clicks.sort_by(|left, right| left.t.total_cmp(&right.t));
    presses.sort_by(|left, right| left.down.total_cmp(&right.down));
    (clicks, presses)
}

fn nearby_stream_position(timeline: &RecordingTimeline, at_us: i64) -> Option<(f64, f64)> {
    let positions = timeline.stream_cursor_positions_at(at_us);
    // The stream only reports the cursor when it moves or the screen changes,
    // so a click after the pointer has been still has no sample close by. The
    // track already confirmed the cursor was visible in the frame at the
    // press, and it cannot move without producing a sample, so the last one
    // before the press is where it was.
    positions
        .iter()
        .filter(|(_, _, sampled_at_us)| {
            sampled_at_us.abs_diff(at_us) <= CLICK_SAMPLE_TOLERANCE_US as u64
        })
        .min_by_key(|(_, _, sampled_at_us)| sampled_at_us.abs_diff(at_us))
        .or_else(|| {
            positions
                .iter()
                .filter(|(_, _, sampled_at_us)| *sampled_at_us <= at_us)
                .max_by_key(|(_, _, sampled_at_us)| *sampled_at_us)
        })
        .map(|&(x, y, _)| (x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{CaptureRegion, PointerSidecar};

    const NONCE: &str = "0123456789abcdef";

    fn journal_line(message: &str, at_us: i64) -> String {
        format!(r#"{{"MESSAGE":"{message}","__MONOTONIC_TIMESTAMP":"{at_us}"}}"#)
    }

    fn event(at_us: i64, buttons: i32, old_buttons: i32, x: i32, y: i32) -> KwinClickEvent {
        KwinClickEvent {
            at_us,
            buttons,
            old_buttons,
            x,
            y,
        }
    }

    fn timeline(origin_us: i64) -> RecordingTimeline {
        let mut timeline = RecordingTimeline::default();
        timeline.enable_stream_cursor(200, 100);
        let _ = timeline.frame_time_us(origin_us);
        timeline
    }

    #[test]
    fn parses_a_journal_click_line() {
        let line = journal_line("APEXSHOT_CLICK 0123456789abcdef 1 0 120 240", 9_000_001);
        assert_eq!(
            parse_journal_line(&line, NONCE),
            Some(event(9_000_001, 1, 0, 120, 240))
        );
    }

    #[test]
    fn accepts_a_numeric_journal_timestamp() {
        let line =
            format!(r#"{{"MESSAGE":"APEXSHOT_CLICK {NONCE} 2 0 5 6","__MONOTONIC_TIMESTAMP":42}}"#);
        assert_eq!(
            parse_journal_line(&line, NONCE).map(|event| event.at_us),
            Some(42)
        );
    }

    #[test]
    fn ignores_another_sessions_nonce() {
        let line = journal_line("APEXSHOT_CLICK fedcba9876543210 1 0 120 240", 9_000_001);
        assert!(parse_journal_line(&line, NONCE).is_none());
        // A nonce that only shares a prefix must not match either.
        let prefix_nonce = "0123";
        let longer = journal_line("APEXSHOT_CLICK 0123456789abcdef 1 0 1 2", 9);
        assert!(parse_journal_line(&longer, prefix_nonce).is_none());
    }

    #[test]
    fn ignores_non_json_and_incomplete_lines() {
        assert!(parse_journal_line("APEXSHOT_CLICK 0123456789abcdef 1 0 120 240", NONCE).is_none());
        assert!(parse_journal_line(&journal_line("unrelated log line", 9), NONCE).is_none());
        assert!(parse_journal_line(
            &journal_line("APEXSHOT_CLICK 0123456789abcdef 1 0", 9),
            NONCE
        )
        .is_none());
        assert!(parse_journal_line(
            &journal_line("APEXSHOT_CLICK 0123456789abcdef 1 0 120 240 extra", 9),
            NONCE
        )
        .is_none());
        let missing_timestamp = format!(r#"{{"MESSAGE":"APEXSHOT_CLICK {NONCE} 1 0 120 240"}}"#);
        assert!(parse_journal_line(&missing_timestamp, NONCE).is_none());
        let missing_message = r#"{"__MONOTONIC_TIMESTAMP":"9"}"#;
        assert!(parse_journal_line(missing_message, NONCE).is_none());
    }

    #[test]
    fn a_left_click_becomes_one_press() {
        let presses =
            presses_from_events(&[event(1_000, 1, 0, 10, 20), event(1_120, 0, 1, 10, 20)]);
        assert_eq!(
            presses,
            vec![RawClick {
                down_us: 1_000,
                up_us: Some(1_120),
                button: 1,
                dragged: false,
            }]
        );
    }

    #[test]
    fn a_right_click_maps_to_the_stored_right_button() {
        let presses =
            presses_from_events(&[event(1_000, 2, 0, 10, 20), event(1_100, 0, 2, 10, 20)]);
        assert_eq!(
            presses,
            vec![RawClick {
                down_us: 1_000,
                up_us: Some(1_100),
                button: 3,
                dragged: false,
            }]
        );
    }

    #[test]
    fn a_middle_click_maps_to_the_stored_middle_button() {
        let presses =
            presses_from_events(&[event(1_000, 4, 0, 10, 20), event(1_100, 0, 4, 10, 20)]);
        assert_eq!(
            presses,
            vec![RawClick {
                down_us: 1_000,
                up_us: Some(1_100),
                button: 2,
                dragged: false,
            }]
        );
    }

    #[test]
    fn overlapping_left_and_right_buttons_become_two_presses() {
        let presses = presses_from_events(&[
            event(1_000, 1, 0, 10, 20),
            event(1_050, 3, 1, 10, 20),
            event(1_100, 2, 3, 10, 20),
            event(1_150, 0, 2, 10, 20),
        ]);
        assert_eq!(
            presses,
            vec![
                RawClick {
                    down_us: 1_000,
                    up_us: Some(1_100),
                    button: 1,
                    dragged: false,
                },
                RawClick {
                    down_us: 1_050,
                    up_us: Some(1_150),
                    button: 3,
                    dragged: false,
                },
            ]
        );
    }

    #[test]
    fn movement_before_release_marks_a_drag() {
        let dragged =
            presses_from_events(&[event(1_000, 1, 0, 10, 20), event(1_300, 0, 1, 15, 20)]);
        assert!(dragged[0].dragged);

        let steady = presses_from_events(&[event(1_000, 1, 0, 10, 20), event(1_300, 0, 1, 12, 22)]);
        assert!(!steady[0].dragged);
    }

    #[test]
    fn a_button_still_held_at_stop_has_no_release() {
        let presses = presses_from_events(&[event(1_000, 1, 0, 10, 20)]);
        assert_eq!(
            presses,
            vec![RawClick {
                down_us: 1_000,
                up_us: None,
                button: 1,
                dragged: false,
            }]
        );
    }

    #[test]
    fn a_click_inside_the_frame_uses_the_nearby_cursor_sample() {
        let mut timeline = timeline(1_000_000);
        timeline.record_stream_cursor(1_500_000, Some((12.0, 18.0)));
        let raw = vec![RawClick {
            down_us: 1_520_000,
            up_us: Some(1_600_000),
            button: 1,
            dragged: false,
        }];

        let (clicks, presses) = clicks_and_presses(&raw, &timeline);

        assert_eq!(clicks.len(), 1);
        assert!((clicks[0].t - 0.52).abs() < 1e-9);
        assert_eq!(clicks[0].x, 12.0);
        assert_eq!(clicks[0].y, 18.0);
        assert_eq!(clicks[0].button, 1);
        assert_eq!(presses.len(), 1);
        assert!((presses[0].down - 0.52).abs() < 1e-9);
        assert!((presses[0].up - 0.6).abs() < 1e-9);
    }

    #[test]
    fn a_click_after_the_cursor_rests_uses_its_last_position() {
        // A still cursor on a still screen produces no samples, so the last
        // visible one before the press is where the click happened.
        let mut timeline = timeline(1_000_000);
        timeline.record_stream_cursor(1_000_000, Some((12.0, 18.0)));
        let raw = vec![RawClick {
            down_us: 1_500_000,
            up_us: Some(1_550_000),
            button: 1,
            dragged: false,
        }];

        let (clicks, presses) = clicks_and_presses(&raw, &timeline);

        assert_eq!(clicks.len(), 1);
        assert_eq!((clicks[0].x, clicks[0].y), (12.0, 18.0));
        assert_eq!(presses.len(), 1);
    }

    #[test]
    fn a_click_while_the_cursor_is_outside_the_frame_is_dropped() {
        let mut timeline = timeline(1_000_000);
        timeline.record_stream_cursor(1_000_000, Some((12.0, 18.0)));
        timeline.record_stream_cursor(1_200_000, None);
        let raw = vec![RawClick {
            down_us: 1_500_000,
            up_us: Some(1_550_000),
            button: 1,
            dragged: false,
        }];

        let (clicks, presses) = clicks_and_presses(&raw, &timeline);

        assert!(clicks.is_empty());
        assert!(presses.is_empty());
    }

    #[test]
    fn a_click_during_a_pause_is_dropped_by_the_timeline_rebase() {
        let mut timeline = timeline(1_000_000);
        timeline.record_stream_cursor(1_500_000, Some((12.0, 18.0)));
        timeline.record_stream_cursor(2_500_000, Some((20.0, 30.0)));
        timeline.pause(2_000_000);
        timeline.resume(3_000_000);
        timeline.finish(4_000_000);
        let raw = vec![
            RawClick {
                down_us: 1_500_000,
                up_us: Some(1_600_000),
                button: 1,
                dragged: false,
            },
            RawClick {
                down_us: 2_500_000,
                up_us: Some(2_600_000),
                button: 3,
                dragged: false,
            },
        ];

        let (clicks, presses) = clicks_and_presses(&raw, &timeline);
        assert_eq!(clicks.len(), 2);
        assert_eq!(presses.len(), 2);

        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        assert!(timeline.add_stream_cursor_to_sidecar(&mut sidecar, 1_000_000));
        sidecar.clicks = clicks;
        sidecar.presses = presses;
        assert!(timeline.rebase(&mut sidecar));

        // The click inside the removed pause interval is gone; the earlier one
        // keeps its own time on the retained recording clock.
        assert_eq!(sidecar.clicks.len(), 1);
        assert!((sidecar.clicks[0].t - 0.5).abs() < 1e-9);
        assert_eq!(sidecar.presses.len(), 1);
        assert!((sidecar.presses[0].down - 0.5).abs() < 1e-9);
    }
}
