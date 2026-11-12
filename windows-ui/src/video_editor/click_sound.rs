use crate::recording::editor::sidecar::{PointerSidecar, PressSample};
use std::collections::BTreeMap;

pub(super) const WAV: &[u8] = include_bytes!("../../../assets/sounds/click-soft.wav");

const CLICK_PAIR_WINDOW_SECONDS: f64 = 0.25;
const LEGACY_CLICK_MATCH_SECONDS: f64 = 1e-6;
const MAX_SPEED: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Click,
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Event {
    pub kind: Kind,
    pub source_time: f64,
    pub composition_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Span {
    pub source_start: f64,
    pub source_end: f64,
    pub composition_start: f64,
    pub speed: f64,
}

pub(super) fn events(sidecar: &PointerSidecar, spans: &[Span]) -> Vec<Event> {
    let presses: Vec<PressSample> = sidecar
        .presses
        .iter()
        .copied()
        .filter(|press| is_time(press.down) && is_time(press.up) && press.up >= press.down)
        .collect();

    let mut downs: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
    let mut source_events: Vec<(Kind, f64)> = Vec::new();
    for press in &presses {
        downs.entry(press.button).or_default().push(press.down);
        if press.up - press.down < CLICK_PAIR_WINDOW_SECONDS {
            source_events.push((Kind::Click, press.down));
        } else {
            source_events.push((Kind::Down, press.down));
            source_events.push((Kind::Up, press.up));
        }
    }
    for times in downs.values_mut() {
        times.sort_by(f64::total_cmp);
    }
    for click in sidecar.clicks.iter().filter(|click| is_time(click.t)) {
        let matched = downs.get(&click.button).is_some_and(|times| {
            let window = 2.0 * LEGACY_CLICK_MATCH_SECONDS;
            let first = times.partition_point(|&down| down < click.t - window);
            times[first..]
                .iter()
                .take_while(|&&down| down <= click.t + window)
                .any(|&down| (down - click.t).abs() <= LEGACY_CLICK_MATCH_SECONDS)
        });
        if !matched {
            source_events.push((Kind::Click, click.t));
        }
    }

    let mut mapped: Vec<Event> = source_events
        .into_iter()
        .filter_map(|(kind, source_time)| {
            let span = spans.iter().find(|span| span_covers(span, source_time))?;
            let composition_time =
                span.composition_start + (source_time - span.source_start) / span.speed;
            (composition_time.is_finite() && composition_time > 0.0).then_some(Event {
                kind,
                source_time,
                composition_time,
            })
        })
        .collect();
    mapped.sort_by(|a, b| {
        a.composition_time
            .total_cmp(&b.composition_time)
            .then(a.source_time.total_cmp(&b.source_time))
    });
    mapped
}

#[cfg(target_os = "windows")]
pub(super) fn preview(volume: f64) {
    let volume = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    };
    std::thread::spawn(move || play(volume));
}

#[cfg(not(target_os = "windows"))]
pub(super) fn preview(_volume: f64) {}

#[cfg(target_os = "windows")]
fn play(volume: f64) {
    const SND_MEMORY: u32 = 0x0004;
    const SND_NODEFAULT: u32 = 0x0002;
    let Some(sound) = scaled(WAV, volume) else {
        return;
    };
    unsafe {
        PlaySoundW(
            sound.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_NODEFAULT,
        );
    }
}

#[cfg(target_os = "windows")]
fn scaled(wav: &[u8], volume: f64) -> Option<Vec<u8>> {
    const HEADER: usize = 44;
    let valid = wav.len() >= HEADER
        && &wav[0..4] == b"RIFF"
        && &wav[8..12] == b"WAVE"
        && &wav[12..16] == b"fmt "
        && u16::from_le_bytes([wav[34], wav[35]]) == 16
        && &wav[36..40] == b"data";
    if !valid {
        return None;
    }
    let mut out = wav.to_vec();
    for sample in out[HEADER..].as_chunks_mut::<2>().0 {
        let value = f64::from(i16::from_le_bytes([sample[0], sample[1]])) * volume;
        sample.copy_from_slice(&(value.round() as i16).to_le_bytes());
    }
    Some(out)
}

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(sound: *const std::ffi::c_void, module: *mut std::ffi::c_void, flags: u32)
        -> i32;
}

fn is_time(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

fn span_covers(span: &Span, t: f64) -> bool {
    span.source_start.is_finite()
        && span.source_end.is_finite()
        && span.composition_start.is_finite()
        && span.speed.is_finite()
        && span.speed > 0.0
        && span.speed <= MAX_SPEED
        && (span.source_start..span.source_end).contains(&t)
}

#[cfg(test)]
mod tests {
    use super::{events, Event, Kind, Span};
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, PointerSidecar, PressSample,
    };

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 640,
                h: 480,
            },
        )
    }

    fn press(down: f64, up: f64, button: i32) -> PressSample {
        PressSample {
            down,
            up,
            button,
            dragged: false,
        }
    }

    fn click(t: f64, button: i32) -> ClickSample {
        ClickSample {
            t,
            x: 0.0,
            y: 0.0,
            button,
            cursor_id: None,
        }
    }

    fn identity() -> Vec<Span> {
        vec![Span {
            source_start: 0.0,
            source_end: 100.0,
            composition_start: 0.0,
            speed: 1.0,
        }]
    }

    fn event(kind: Kind, source_time: f64, composition_time: f64) -> Event {
        Event {
            kind,
            source_time,
            composition_time,
        }
    }

    #[test]
    fn a_click_at_the_very_start_plays_no_sound() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(0.0, 0.05, 1));
        assert_eq!(events(&sidecar, &identity()), Vec::new());
    }

    #[test]
    fn a_short_press_gives_one_click_at_its_down_time() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.1, 1));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Click, 1.0, 1.0)]
        );
    }

    #[test]
    fn a_long_press_gives_a_down_and_an_up() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 2.0, 1));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Down, 1.0, 1.0), event(Kind::Up, 2.0, 2.0)]
        );
    }

    #[test]
    fn two_close_independent_presses_remain_two_clicks() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.presses.push(press(1.1, 1.15, 1));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Click, 1.0, 1.0), event(Kind::Click, 1.1, 1.1)]
        );
    }

    #[test]
    fn an_unmatched_legacy_click_gives_one_click() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(3.0, 1));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Click, 3.0, 3.0)]
        );
    }

    #[test]
    fn a_legacy_click_matching_a_press_is_suppressed() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(1.0, 1));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Click, 1.0, 1.0)]
        );
    }

    #[test]
    fn a_legacy_click_pairs_only_with_a_press_of_the_same_button() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(1.0, 2));
        assert_eq!(
            events(&sidecar, &identity()),
            vec![event(Kind::Click, 1.0, 1.0), event(Kind::Click, 1.0, 1.0)]
        );
    }

    #[test]
    fn trimmed_presses_map_through_a_double_speed_span() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.presses.push(press(3.0, 4.0, 1));
        sidecar.presses.push(press(5.0, 5.1, 1));
        sidecar.presses.push(press(6.0, 6.05, 1));
        let spans = [Span {
            source_start: 2.0,
            source_end: 6.0,
            composition_start: 0.0,
            speed: 2.0,
        }];
        assert_eq!(
            events(&sidecar, &spans),
            vec![
                event(Kind::Down, 3.0, 0.5),
                event(Kind::Up, 4.0, 1.0),
                event(Kind::Click, 5.0, 1.5),
            ]
        );
    }

    #[test]
    fn a_span_faster_than_two_times_is_skipped() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(1.0, 1.05, 1));
        sidecar.clicks.push(click(2.0, 1));
        let spans = [Span {
            source_start: 0.0,
            source_end: 10.0,
            composition_start: 0.0,
            speed: 3.0,
        }];
        assert_eq!(events(&sidecar, &spans), Vec::new());
    }

    #[test]
    fn malformed_times_and_spans_are_ignored() {
        let mut sidecar = sidecar();
        sidecar.presses.push(press(f64::NAN, 1.0, 1));
        sidecar.presses.push(press(-1.0, 0.5, 1));
        sidecar.presses.push(press(2.0, 1.0, 1));
        sidecar.presses.push(press(1.0, f64::INFINITY, 1));
        sidecar.clicks.push(click(f64::NAN, 1));
        sidecar.clicks.push(click(-2.0, 1));
        let spans = [
            Span {
                source_start: 0.0,
                source_end: f64::NAN,
                composition_start: 0.0,
                speed: 1.0,
            },
            Span {
                source_start: 0.0,
                source_end: 10.0,
                composition_start: 0.0,
                speed: 0.0,
            },
            Span {
                source_start: 0.0,
                source_end: 10.0,
                composition_start: f64::NAN,
                speed: 1.0,
            },
        ];
        assert_eq!(events(&sidecar, &spans), Vec::new());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn the_preview_volume_quiets_only_the_bundled_pcm() {
        use super::{scaled, WAV};

        const HEADER: usize = 44;
        let peaks = |bytes: &[u8]| {
            bytes[HEADER..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| i32::from(i16::from_le_bytes(*sample)).abs())
                .max()
                .unwrap_or(0)
        };

        let full = scaled(WAV, 1.0).expect("the bundled click is a 16-bit PCM wav");
        assert_eq!(full.as_slice(), WAV);

        let quiet = scaled(WAV, 0.25).expect("the bundled click is a 16-bit PCM wav");
        assert_eq!(quiet.len(), WAV.len());
        assert_eq!(quiet[..HEADER], WAV[..HEADER]);

        let full_peak = peaks(WAV);
        assert!(full_peak > 0);
        assert!((f64::from(peaks(&quiet)) - f64::from(full_peak) / 4.0).abs() <= 1.0);
    }
}
