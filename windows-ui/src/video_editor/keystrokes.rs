//! Keyboard shortcut captions for the video editor: recorded key presses are
//! grouped into captions, and a caption is drawn as a row of keycaps.

use crate::recording::editor::sidecar::KeySample;
use gtk4::cairo::Context;
use std::f64::consts::{FRAC_PI_2, PI};

const LEAD: f64 = 0.1;
const TAIL: f64 = 1.5;
const MIN_DISPLAY: f64 = 0.8;
const MOD_LOOKBACK: f64 = 3.0;
const SAME_SHORTCUT_GAP: f64 = 2.0;
/// A shifted key followed this soon by a plain key is typing, not a shortcut.
const SHIFT_TYPING_GAP: f64 = 1.0;
/// Two plain letters or digits this close are typing while single letters are shown.
const LETTER_RUN_GAP: f64 = 1.0;

/// A key's height as a share of the frame height.
const KEY_HEIGHT: f64 = 0.075;
const BOTTOM_INSET: f64 = 0.033;
/// Space between the pill's edge and its keys.
const PILL_PADDING: f64 = 6.0 / 34.0;
/// The pill's corner radius.
const PILL_RADIUS: f64 = 9.0 / 34.0;
/// A key's corner radius.
const KEY_RADIUS: f64 = 5.0 / 34.0;
/// Space either side of a key's label.
const KEY_PADDING: f64 = 14.0 / 34.0;
/// Space between two keys.
const GAP: f64 = 6.0 / 34.0;
/// The label's size.
const FONT_SIZE: f64 = 12.0 / 34.0 * 1.25;
/// The pill's fill, `#0f0f0f`, slightly see-through over footage.
const PILL_FILL: (f64, f64, f64, f64) = (15.0 / 255.0, 15.0 / 255.0, 15.0 / 255.0, 0.92);
/// A key's fill over the pill.
const KEY_FILL: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 0.08);
/// The label colour.
const LABEL_COLOR: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 0.86);
const ROW_WIDTH_SHARE: f64 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Settings {
    #[serde(default)]
    pub shortcuts: bool,
    #[serde(default)]
    pub show_single_letters: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Caption {
    pub start: f64,
    pub end: f64,
    pub keys: Vec<String>,
    pub count: usize,
}

struct Group {
    keys: Vec<String>,
    mask: u8,
    name: &'static str,
    first_t: f64,
    last_down: f64,
    count: usize,
    first_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Control,
    Alt,
    Shift,
    Win,
}

impl Modifier {
    const ALL: [Modifier; 4] = [Self::Control, Self::Alt, Self::Shift, Self::Win];

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "control" => Some(Self::Control),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "win" => Some(Self::Win),
            _ => None,
        }
    }

    fn from_vk(key: i32) -> Option<Self> {
        match key {
            16 | 160 | 161 => Some(Self::Shift),
            17 | 162 | 163 => Some(Self::Control),
            18 | 164 | 165 => Some(Self::Alt),
            91 | 92 => Some(Self::Win),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Control => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Win => "Win",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

pub(super) fn captions(keys: &[KeySample], show_single_letters: bool) -> Vec<Caption> {
    let mut events: Vec<&KeySample> = keys
        .iter()
        .filter(|event| event.t.is_finite() && event.t >= 0.0)
        .filter(|event| matches!(event.kind.as_str(), "key_down" | "key_up"))
        .collect();
    events.sort_by(|a, b| a.t.total_cmp(&b.t));

    let entries: Vec<(usize, f64, u8, &'static str)> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == "key_down" && Modifier::from_vk(event.key).is_none())
        .filter_map(|(index, event)| {
            let name = vk_name(event.key);
            (!name.is_empty()).then(|| (index, event.t, modifier_mask(&event.modifiers), name))
        })
        .collect();

    let kept: Vec<(usize, f64, u8, &'static str)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, &(index, t, mask, name))| {
            if mask & Modifier::Shift.bit() != 0 {
                if let Some(&(_, next_t, next_mask, _)) = entries.get(i + 1) {
                    if next_mask == 0 && next_t - t < SHIFT_TYPING_GAP {
                        return None;
                    }
                }
            }
            if !show_single_letters && mask == 0 && !shown_alone(name) {
                return None;
            }
            Some((index, t, mask, name))
        })
        .collect();

    let mut groups: Vec<Group> = Vec::new();
    for (index, t, mask, name) in kept {
        let shortcut: Vec<String> = Modifier::ALL
            .iter()
            .filter(|modifier| mask & modifier.bit() != 0)
            .map(|modifier| modifier.label().to_owned())
            .chain(std::iter::once(name.to_owned()))
            .collect();

        if show_single_letters {
            let letter = |n: &str| n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric());
            if let Some(last) = groups.last() {
                if last.mask == 0
                    && letter(last.name)
                    && mask == 0
                    && letter(name)
                    && t - last.first_t < LETTER_RUN_GAP
                {
                    groups.pop();
                    continue;
                }
            }
        }

        if let Some(last) = groups.last_mut() {
            if last.keys == shortcut && t - last.last_down <= SAME_SHORTCUT_GAP {
                last.count += 1;
                last.last_down = t;
                continue;
            }
        }

        groups.push(Group {
            keys: shortcut,
            mask,
            name,
            first_t: t,
            last_down: t,
            count: 1,
            first_index: index,
        });
    }

    let mut captions: Vec<Caption> = Vec::with_capacity(groups.len());
    let mut floor = f64::NEG_INFINITY;
    for group in groups {
        let candidate = caption_start(
            &events[..group.first_index],
            group.first_t,
            group.mask,
            floor,
        );
        let start = match captions.last_mut() {
            Some(previous) => {
                let start = (group.first_t - LEAD)
                    .min(candidate.max(previous.start + MIN_DISPLAY))
                    .max(0.0);
                previous.end = previous.end.min(start).max(previous.start);
                start
            }
            None => candidate,
        };
        floor = group.last_down;
        captions.push(Caption {
            start,
            end: group.last_down + TAIL,
            keys: group.keys,
            count: group.count,
        });
    }
    captions
}

/// Keys that still earn a caption when pressed alone: editing and navigation
/// keys and the function keys. Letters, digits, Enter, Space, Tab and Escape
/// pressed alone are typing.
fn shown_alone(name: &str) -> bool {
    matches!(
        name,
        "Backspace"
            | "Delete"
            | "Left"
            | "Up"
            | "Right"
            | "Down"
            | "Home"
            | "End"
            | "PageUp"
            | "PageDown"
    ) || (name.len() >= 2 && name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()))
}

fn modifier_mask(names: &[String]) -> u8 {
    names
        .iter()
        .filter_map(|name| Modifier::from_name(name))
        .fold(0, |mask, modifier| mask | modifier.bit())
}

fn caption_start(earlier: &[&KeySample], t: f64, mask: u8, floor: f64) -> f64 {
    let lowest = (t - MOD_LOOKBACK).max(floor);
    let mut start = (t - LEAD).max(0.0);
    for modifier in Modifier::ALL {
        if mask & modifier.bit() == 0 {
            continue;
        }
        if let Some(hold) = modifier_hold(earlier, lowest, modifier) {
            start = start.min((hold - LEAD).max(0.0));
        }
    }
    start
}

fn modifier_hold(earlier: &[&KeySample], lowest: f64, modifier: Modifier) -> Option<f64> {
    let mut hold = None;
    for event in earlier.iter().rev() {
        if event.t < lowest {
            break;
        }
        if Modifier::from_vk(event.key) != Some(modifier) {
            continue;
        }
        if event.kind == "key_up" {
            break;
        }
        hold = Some(event.t);
    }
    hold
}

pub(super) fn vk_name(key: i32) -> &'static str {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FUNCTION_KEYS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];

    if let Some(modifier) = Modifier::from_vk(key) {
        return modifier.label();
    }
    match key {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2D => "Insert",
        0x2E => "Delete",
        0x30..=0x39 => DIGITS[(key - 0x30) as usize],
        0x41..=0x5A => LETTERS[(key - 0x41) as usize],
        0x70..=0x87 => FUNCTION_KEYS[(key - 0x70) as usize],
        _ => "",
    }
}

pub(super) fn draw(cr: &Context, width: f64, height: f64, captions: &[Caption], time: f64) {
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        return;
    }
    let Some(caption) = captions
        .iter()
        .find(|caption| caption.start <= time && time < caption.end)
    else {
        return;
    };
    if cr.save().is_err() {
        return;
    }
    draw_row(cr, width, height, caption);
    let _ = cr.restore();
}

fn draw_row(cr: &Context, width: f64, height: f64, caption: &Caption) {
    let mut items = caption.keys.clone();
    if caption.count > 1 {
        items.push(format!("×{}", caption.count));
    }

    let base_height = KEY_HEIGHT * height;
    let natural = keycaps(cr, &items, base_height);
    let natural_pill_width =
        row_width(&natural, base_height * GAP) + 2.0 * base_height * PILL_PADDING;
    let scale = (ROW_WIDTH_SHARE * width / natural_pill_width).min(1.0);

    let key_height = base_height * scale;
    let caps = keycaps(cr, &items, key_height);
    let gap = key_height * GAP;
    let pad = key_height * PILL_PADDING;
    let pill_width = row_width(&caps, gap) + 2.0 * pad;
    let pill_height = key_height + 2.0 * pad;

    let pill_top = height * (1.0 - BOTTOM_INSET) - pill_height;
    let pill_left = (width - pill_width) / 2.0;

    rounded_rect(
        cr,
        pill_left,
        pill_top,
        pill_width,
        pill_height,
        key_height * PILL_RADIUS,
    );
    cr.set_source_rgba(PILL_FILL.0, PILL_FILL.1, PILL_FILL.2, PILL_FILL.3);
    let _ = cr.fill();

    let mut x = pill_left + pad;
    let y = pill_top + pad;
    for (label, cap_width) in &caps {
        rounded_rect(cr, x, y, *cap_width, key_height, key_height * KEY_RADIUS);
        cr.set_source_rgba(KEY_FILL.0, KEY_FILL.1, KEY_FILL.2, KEY_FILL.3);
        let _ = cr.fill();

        let (text_width, text_height) = label.size(cr);
        cr.set_source_rgba(LABEL_COLOR.0, LABEL_COLOR.1, LABEL_COLOR.2, LABEL_COLOR.3);
        label.show(
            cr,
            x + (cap_width - text_width) / 2.0,
            y + (key_height - text_height) / 2.0,
        );
        x += cap_width + gap;
    }
}

fn keycaps(cr: &Context, items: &[String], key_height: f64) -> Vec<(Label, f64)> {
    let padding = key_height * KEY_PADDING;
    items
        .iter()
        .map(|item| {
            let label = Label::new(cr, item, key_height * FONT_SIZE);
            let (text_width, _) = label.size(cr);
            (label, (text_width + 2.0 * padding).max(key_height))
        })
        .collect()
}

fn row_width(caps: &[(Label, f64)], gap: f64) -> f64 {
    let widths: f64 = caps.iter().map(|(_, width)| width).sum();
    widths + gap * caps.len().saturating_sub(1) as f64
}

fn rounded_rect(cr: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        FRAC_PI_2,
    );
    cr.arc(x + radius, y + height - radius, radius, FRAC_PI_2, PI);
    cr.arc(x + radius, y + radius, radius, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

#[cfg(windows)]
struct Label(gtk4::pango::Layout);

#[cfg(windows)]
impl Label {
    fn new(cr: &Context, text: &str, font_px: f64) -> Self {
        let layout = pangocairo::functions::create_layout(cr);
        let mut font = gtk4::pango::FontDescription::new();
        font.set_family("Segoe UI");
        font.set_weight(gtk4::pango::Weight::Semibold);
        font.set_absolute_size(font_px * f64::from(gtk4::pango::SCALE));
        layout.set_font_description(Some(&font));
        layout.set_text(text);
        Self(layout)
    }

    fn size(&self, _cr: &Context) -> (f64, f64) {
        let (width, height) = self.0.pixel_size();
        (f64::from(width), f64::from(height))
    }

    fn show(&self, cr: &Context, x: f64, y: f64) {
        cr.move_to(x, y);
        pangocairo::functions::show_layout(cr, &self.0);
    }
}

#[cfg(not(windows))]
struct Label {
    text: String,
    font_px: f64,
}

#[cfg(not(windows))]
impl Label {
    fn new(_cr: &Context, text: &str, font_px: f64) -> Self {
        Self {
            text: text.to_owned(),
            font_px,
        }
    }

    fn select(&self, cr: &Context) {
        cr.select_font_face(
            "Sans",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        cr.set_font_size(self.font_px);
    }

    fn size(&self, cr: &Context) -> (f64, f64) {
        self.select(cr);
        let width = cr
            .text_extents(&self.text)
            .map_or(0.0, |extents| extents.x_advance());
        let height = cr
            .font_extents()
            .map_or(self.font_px, |extents| extents.height());
        (width, height)
    }

    fn show(&self, cr: &Context, x: f64, y: f64) {
        self.select(cr);
        let ascent = cr
            .font_extents()
            .map_or(self.font_px, |extents| extents.ascent());
        cr.move_to(x, y + ascent);
        let _ = cr.show_text(&self.text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VK_SHIFT: i32 = 16;
    const VK_CONTROL: i32 = 17;
    const VK_A: i32 = 65;
    const VK_J: i32 = 74;
    const VK_K: i32 = 75;
    const VK_ENTER: i32 = 13;
    const VK_FIVE: i32 = 53;
    const VK_UNMAPPED: i32 = 255;

    fn event(t: f64, kind: &str, key: i32, modifiers: &[&str]) -> KeySample {
        KeySample {
            t,
            kind: kind.to_owned(),
            key,
            scan_code: 0,
            modifiers: modifiers.iter().map(|name| (*name).to_owned()).collect(),
            cursor_id: None,
        }
    }

    fn down(t: f64, key: i32, modifiers: &[&str]) -> KeySample {
        event(t, "key_down", key, modifiers)
    }

    fn up(t: f64, key: i32) -> KeySample {
        event(t, "key_up", key, &[])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn ctrl_shift_k_is_one_caption_from_first_modifier_to_tail() {
        let events = [
            down(1.0, VK_CONTROL, &[]),
            down(1.1, VK_SHIFT, &["control"]),
            down(1.2, VK_K, &["control", "shift"]),
            up(1.25, VK_K),
            up(1.3, VK_SHIFT),
            up(1.35, VK_CONTROL),
        ];
        let out = captions(&events, false);
        assert_eq!(out.len(), 1);
        assert!(close(out[0].start, 0.9));
        assert!(close(out[0].end, 2.7));
        assert_eq!(out[0].keys, ["Ctrl", "Shift", "K"]);
        assert_eq!(out[0].count, 1);
    }

    #[test]
    fn key_up_does_not_extend_the_caption() {
        let events = [
            down(1.0, VK_K, &["control"]),
            up(4.0, VK_K),
            up(4.1, VK_CONTROL),
        ];
        let out = captions(&events, false);
        assert_eq!(out.len(), 1);
        assert!(close(out[0].end, 2.5));
    }

    #[test]
    fn repeated_shortcut_within_gap_merges_and_counts() {
        let events = [down(1.0, VK_K, &["control"]), down(1.5, VK_K, &["control"])];
        let out = captions(&events, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].count, 2);
        assert!(close(out[0].end, 3.0));
    }

    #[test]
    fn same_shortcut_one_and_a_half_seconds_apart_merges() {
        let events = [down(1.0, VK_K, &["control"]), down(2.5, VK_K, &["control"])];
        let out = captions(&events, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].count, 2);
    }

    #[test]
    fn same_shortcut_beyond_the_gap_starts_a_new_caption() {
        let events = [down(1.0, VK_K, &["control"]), down(3.5, VK_K, &["control"])];
        assert_eq!(captions(&events, false).len(), 2);
    }

    #[test]
    fn distinct_shortcuts_one_and_a_half_seconds_apart_separate() {
        let events = [down(1.0, VK_K, &["control"]), down(2.5, VK_J, &["control"])];
        let out = captions(&events, false);
        assert_eq!(out.len(), 2);
        assert!(close(out[0].end, 2.4));
        assert!(close(out[1].start, 2.4));
        assert_eq!(out[1].keys, ["Ctrl", "J"]);
    }

    #[test]
    fn lone_keys_follow_the_shown_alone_list() {
        for key in [VK_A, VK_FIVE, VK_ENTER, 32, 9, 27] {
            assert!(
                captions(&[down(1.0, key, &[])], false).is_empty(),
                "virtual key {key}"
            );
        }

        for (key, name) in [(46, "Delete"), (37, "Left"), (116, "F5"), (8, "Backspace")] {
            let out = captions(&[down(1.0, key, &[])], false);
            assert_eq!(out.len(), 1, "virtual key {key}");
            assert_eq!(out[0].keys, [name]);
        }

        let shortcut = [down(1.0, VK_A, &["control"])];
        assert_eq!(captions(&shortcut, false).len(), 1);

        let enter = [down(1.0, VK_ENTER, &[])];
        assert_eq!(captions(&enter, true).len(), 1);
    }

    #[test]
    fn a_shifted_letter_is_typing_only_when_plain_typing_follows() {
        let alone = [down(1.0, VK_A, &["shift"])];
        let out = captions(&alone, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].keys, ["Shift", "A"]);

        let typed = [down(1.0, VK_A, &["shift"]), down(1.5, VK_J, &[])];
        assert!(captions(&typed, false).is_empty());

        let apart = [down(1.0, VK_A, &["shift"]), down(2.5, VK_J, &[])];
        let out = captions(&apart, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].keys, ["Shift", "A"]);

        let shortcut = [down(1.0, VK_A, &["control", "shift"])];
        assert_eq!(captions(&shortcut, false)[0].keys, ["Ctrl", "Shift", "A"]);
    }

    #[test]
    fn fast_letter_runs_hide_while_single_letters_are_shown() {
        let fast = [down(1.0, VK_A, &[]), down(1.4, VK_J, &[])];
        assert!(captions(&fast, true).is_empty());

        let apart = [down(1.0, VK_A, &[]), down(2.5, VK_J, &[])];
        assert_eq!(captions(&apart, true).len(), 2);
    }

    #[test]
    fn a_modifier_held_before_the_previous_caption_does_not_leak_into_the_next() {
        let events = [
            down(0.0, VK_CONTROL, &[]),
            down(1.0, VK_K, &["control"]),
            down(1.5, VK_J, &["control"]),
        ];
        let out = captions(&events, false);
        assert_eq!(out.len(), 2);
        assert!(close(out[1].start, 1.4));
    }

    #[test]
    fn modifier_downs_alone_create_no_caption() {
        let events = [
            down(1.0, VK_CONTROL, &[]),
            down(1.1, VK_SHIFT, &["control"]),
        ];
        assert!(captions(&events, false).is_empty());
    }

    #[test]
    fn empty_or_invalid_input_yields_no_captions() {
        assert!(captions(&[], false).is_empty());
        let invalid = [
            down(f64::NAN, VK_K, &["control"]),
            down(-1.0, VK_K, &["control"]),
            down(f64::INFINITY, VK_K, &["control"]),
            down(1.0, VK_UNMAPPED, &["control"]),
            event(1.0, "mouse_down", VK_K, &["control"]),
            up(1.0, VK_K),
        ];
        assert!(captions(&invalid, true).is_empty());
    }

    #[test]
    fn modifier_hold_starts_the_caption_and_respects_release() {
        let events = [
            down(0.0, VK_CONTROL, &[]),
            up(0.5, VK_CONTROL),
            down(2.0, VK_CONTROL, &[]),
            down(2.2, VK_K, &["control"]),
        ];
        let out = captions(&events, false);
        assert!(close(out[0].start, 1.9));
    }

    #[test]
    fn modifier_held_longer_than_the_lookback_is_ignored() {
        let events = [down(0.0, VK_CONTROL, &[]), down(4.0, VK_K, &["control"])];
        let out = captions(&events, false);
        assert!(close(out[0].start, 3.9));
    }

    #[test]
    fn vk_name_maps_supported_keys_and_modifiers() {
        let cases: [(i32, &str); 28] = [
            (65, "A"),
            (90, "Z"),
            (48, "0"),
            (57, "9"),
            (112, "F1"),
            (135, "F24"),
            (136, ""),
            (37, "Left"),
            (38, "Up"),
            (39, "Right"),
            (40, "Down"),
            (32, "Space"),
            (13, "Enter"),
            (9, "Tab"),
            (27, "Escape"),
            (8, "Backspace"),
            (46, "Delete"),
            (16, "Shift"),
            (160, "Shift"),
            (161, "Shift"),
            (17, "Ctrl"),
            (162, "Ctrl"),
            (163, "Ctrl"),
            (18, "Alt"),
            (164, "Alt"),
            (165, "Alt"),
            (91, "Win"),
            (92, "Win"),
        ];
        for (key, name) in cases {
            assert_eq!(vk_name(key), name, "virtual key {key}");
        }
    }

    fn paints(caption: &Caption, time: f64) -> bool {
        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 320, 180)
            .expect("surface");
        let cr = gtk4::cairo::Context::new(&surface).expect("context");
        draw(&cr, 320.0, 180.0, std::slice::from_ref(caption), time);
        drop(cr);
        surface.flush();
        let data = surface.data().expect("pixels");
        data.iter().any(|byte| *byte != 0)
    }

    #[test]
    fn draw_paints_only_while_the_caption_is_active() {
        let Some((active, inactive)) = crate::test_support::with_gtk(|| {
            let caption = Caption {
                start: 1.0,
                end: 2.0,
                keys: vec!["Ctrl".to_owned(), "K".to_owned()],
                count: 2,
            };
            (paints(&caption, 1.5), paints(&caption, 3.0))
        }) else {
            return;
        };
        assert!(active);
        assert!(!inactive);
    }
}
