//! Compact GTK capture toolbar rendered from the C++ toolbar's own geometry.
//!
//! Drawing it with Cairo keeps GTK themes from substituting legacy widgets or
//! text glyphs for the C++ visual contract.

use super::capture_menu_icons as icons;
use super::monitor_picker::{find_monitor_at, MonitorChoice};
use gtk4::{
    cairo, gdk, glib, prelude::*, CssProvider, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureClick, Window,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use pango::prelude::FontMapExt;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMenuAction {
    Cancel,
    Display,
    Window,
    Area,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMenuResult {
    pub action: CaptureMenuAction,
    pub recording: bool,
    pub ocr: bool,
    pub timer_seconds: u8,
    pub microphone: bool,
    pub speaker: bool,
    /// Linux: Pulse sources chosen in the device menus. Empty keeps the saved
    /// choice, then the server default.
    #[serde(default)]
    pub mic_device: String,
    #[serde(default)]
    pub speaker_device: String,
}

impl Default for CaptureMenuResult {
    fn default() -> Self {
        Self {
            action: CaptureMenuAction::Cancel,
            recording: false,
            ocr: false,
            timer_seconds: 0,
            microphone: false,
            speaker: false,
            mic_device: String::new(),
            speaker_device: String::new(),
        }
    }
}

#[derive(Clone, Default)]
struct MenuState {
    recording: bool,
    ocr: bool,
    timer_seconds: u8,
    microphone: bool,
    speaker: bool,
    mic_device: String,
    speaker_device: String,
}

impl MenuState {
    fn result(self, action: CaptureMenuAction) -> CaptureMenuResult {
        CaptureMenuResult {
            action,
            recording: self.recording,
            ocr: self.ocr,
            timer_seconds: self.timer_seconds,
            microphone: self.microphone,
            speaker: self.speaker,
            mic_device: self.mic_device,
            speaker_device: self.speaker_device,
        }
    }
}

const ACCENT: (f64, f64, f64) = (255.0, 102.0, 0.0);
const PANEL_HEIGHT: f64 = 58.0;
const OUTER_PAD: f64 = 6.0;
const ITEM_HEIGHT: f64 = 44.0;
const SEPARATOR: f64 = 12.0;
const ICON_SIZE: f64 = 17.0;
const PANEL_RADIUS: f64 = 13.0;
const ICON_PAD: f64 = 12.0;
const ICON_GAP: f64 = 8.0;
const CLOSE_WIDTH: f64 = 44.0;
const LABEL_PX: f64 = 14.0;
const LABELS: [&str; 8] = [
    "Shot", "Video", "Display", "Window", "Area", "OCR", "Timer", "",
];

#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
    fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

fn separator_after(index: usize) -> bool {
    matches!(index, 1 | 4 | 6)
}

fn item_rects(widths: &[f64; 8]) -> [Rect; 8] {
    let mut rects = [Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }; 8];
    let mut x = OUTER_PAD;
    for (index, rect) in rects.iter_mut().enumerate() {
        *rect = Rect {
            x,
            y: OUTER_PAD,
            width: widths[index],
            height: ITEM_HEIGHT,
        };
        x += widths[index];
        if separator_after(index) {
            x += SEPARATOR;
        }
    }
    rects
}

fn panel_width(rects: &[Rect; 8]) -> f64 {
    let last = rects[7];
    last.x + last.width + OUTER_PAD
}

fn item_at(x: f64, y: f64, rects: &[Rect; 8]) -> Option<usize> {
    (0..8).find(|&i| rects[i].contains(x, y))
}

/// Pango with hinting switched off, matching the C++ toolbar's
/// `PreferNoHinting` so measured and painted widths stay identical.
fn set_font_options(context: &pango::Context) {
    if let Ok(mut options) = cairo::FontOptions::new() {
        options.set_hint_style(cairo::HintStyle::None);
        options.set_hint_metrics(cairo::HintMetrics::Off);
        pangocairo::functions::context_set_font_options(context, Some(&options));
    }
}

fn label_layout(context: &pango::Context, text: &str, weight: pango::Weight) -> pango::Layout {
    let layout = pango::Layout::new(context);
    let mut description = pango::FontDescription::new();
    description.set_family("DM Sans");
    description.set_absolute_size(LABEL_PX * f64::from(pango::SCALE));
    description.set_weight(weight);
    layout.set_font_description(Some(&description));
    layout.set_text(text);
    // The C++ asks for 98 % spacing; Pango takes units, and the 0.55 factor
    // matches how its spacing attribute advances the pen.
    let attributes = pango::AttrList::new();
    attributes.insert(pango::AttrInt::new_letter_spacing(
        -((0.02 * LABEL_PX * 0.55 * f64::from(pango::SCALE)).round() as i32),
    ));
    layout.set_attributes(Some(&attributes));
    layout
}

fn logical_width(layout: &pango::Layout) -> f64 {
    let (_, logical) = layout.extents();
    f64::from(logical.width()) / f64::from(pango::SCALE)
}

fn text_width(context: &pango::Context, text: &str) -> f64 {
    logical_width(&label_layout(context, text, pango::Weight::Medium)).ceil()
}

fn item_widths(state: &MenuState, monitor_width: Option<f64>) -> [f64; 8] {
    let mut widths = plain_item_widths();
    if state.recording {
        let device = device_cell_width(&widths, monitor_width);
        widths[5] = device;
        widths[6] = device;
    }
    widths
}

fn plain_item_widths() -> [f64; 8] {
    let context = pangocairo::FontMap::default().create_context();
    set_font_options(&context);
    let measure = |text: &str| text_width(&context, text);
    let cell = |text: &str| ICON_PAD + ICON_SIZE + ICON_GAP + measure(text) + ICON_PAD;
    [
        cell("Shot"),
        cell("Video"),
        cell("Display"),
        cell("Window"),
        cell("Area"),
        ICON_PAD + ICON_SIZE + ICON_GAP + measure("OCR").max(measure("Mic")) + ICON_PAD,
        ICON_PAD
            + ICON_SIZE
            + ICON_GAP
            + measure("Timer").max(measure("Speaker")).max(measure("10s"))
            + ICON_PAD,
        CLOSE_WIDTH,
    ]
}

/// Mic and Speaker share what the monitor leaves after the fixed cells, the
/// same clamp the C++ toolbar applies when Video is selected.
fn device_cell_width(widths: &[f64; 8], monitor_width: Option<f64>) -> f64 {
    let Some(monitor_width) = monitor_width else {
        return 160.0;
    };
    let separators = (0..8).filter(|&index| separator_after(index)).count() as f64 * SEPARATOR;
    let fixed: f64 = 2.0 * OUTER_PAD
        + separators
        + (0..8)
            .filter(|&index| index != 5 && index != 6)
            .map(|index| widths[index])
            .sum::<f64>();
    ((monitor_width - 24.0 - fixed) / 2.0).clamp(74.0, 160.0)
}

fn item_is_disabled(index: usize, state: &MenuState) -> bool {
    (state.recording && index == 4)
        || (!state.recording && index == 2 && state.ocr)
        || (!state.recording && index == 3 && (state.timer_seconds > 0 || state.ocr))
        || (!state.recording && index == 4 && state.timer_seconds > 0)
        || (!state.recording && index == 5 && state.timer_seconds > 0)
        || (!state.recording && index == 6 && state.ocr)
}

fn item_is_active(index: usize, state: &MenuState) -> bool {
    (index == 0 && !state.recording)
        || (index == 1 && state.recording)
        || (!state.recording && index == 5 && state.ocr)
        || (!state.recording && index == 6 && state.timer_seconds > 0)
        || (state.recording && index == 5 && state.microphone)
        || (state.recording && index == 6 && state.speaker)
}

/// Drawing splits the bar's selected look from the recording phase's device
/// state, exactly as the C++ toolbar does: OCR and Timer must not inherit the
/// last recording's microphone or speaker.
fn drawing_active(index: usize, state: &MenuState) -> bool {
    item_is_active(index, state) && !device_on(index, state)
}

fn device_on(index: usize, state: &MenuState) -> bool {
    state.recording && ((index == 5 && state.microphone) || (index == 6 && state.speaker))
}

fn set_recording_mode(state: &mut MenuState, recording: bool) {
    state.recording = recording;
    if recording {
        state.ocr = false;
        state.timer_seconds = 0;
    }
}

fn rounded_rect(context: &cairo::Context, rect: Rect, radius: f64) {
    let r = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(rect.x + rect.width - r, rect.y + r, r, -PI / 2.0, 0.0);
    context.arc(
        rect.x + rect.width - r,
        rect.y + rect.height - r,
        r,
        0.0,
        PI / 2.0,
    );
    context.arc(rect.x + r, rect.y + rect.height - r, r, PI / 2.0, PI);
    context.arc(rect.x + r, rect.y + r, r, PI, PI * 1.5);
    context.close_path();
}

fn draw_icon(
    context: &cairo::Context,
    glyph: &icons::Glyph,
    center: (f64, f64),
    color: (f64, f64, f64, f64),
) {
    let (min_x, min_y, max_x, max_y) = glyph.bounds;
    let ink_x = (min_x + max_x) / 2.0;
    let ink_y = (min_y + max_y) / 2.0;
    let scale = ICON_SIZE / 1000.0;
    let map = |x: f64, y: f64| {
        (
            center.0 + (x - ink_x) * scale,
            center.1 - (y - ink_y) * scale,
        )
    };
    context.new_path();
    for segment in glyph.segs {
        match *segment {
            icons::Seg::Move(x, y) => {
                let (x, y) = map(x, y);
                context.move_to(x, y);
            }
            icons::Seg::Line(x, y) => {
                let (x, y) = map(x, y);
                context.line_to(x, y);
            }
            icons::Seg::Curve(x1, y1, x2, y2, x3, y3) => {
                let (x1, y1) = map(x1, y1);
                let (x2, y2) = map(x2, y2);
                let (x3, y3) = map(x3, y3);
                context.curve_to(x1, y1, x2, y2, x3, y3);
            }
            icons::Seg::Close => context.close_path(),
        }
    }
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = context.fill();
    context.new_path();
}

fn glyph_for(index: usize, state: &MenuState) -> &'static icons::Glyph {
    match index {
        0 => &icons::CAMERA,
        1 => &icons::VIDEO,
        2 => &icons::MONITOR,
        3 => &icons::APP_WINDOW,
        4 => &icons::SCAN,
        5 if state.recording => &icons::MIC,
        5 => &icons::SCAN_TEXT,
        6 if state.recording => &icons::VOLUME_2,
        6 => &icons::TIMER,
        _ => &icons::X,
    }
}

/// Pulse descriptions for the sources the device cells name. Listed when
/// Video is entered and refreshed whenever a device changes.
#[derive(Default)]
struct DeviceLabels {
    loaded: bool,
    microphones: Vec<crate::recording::AudioInputDevice>,
    outputs: Vec<crate::recording::AudioOutputDevice>,
}

fn load_device_labels(labels: &RefCell<DeviceLabels>, force: bool) {
    if !force && labels.borrow().loaded {
        return;
    }
    let microphones = crate::recording::list_audio_inputs();
    let outputs = crate::recording::list_audio_outputs();
    *labels.borrow_mut() = DeviceLabels {
        loaded: true,
        microphones,
        outputs,
    };
}

/// Relists only when a chosen source is missing from the cache, so a pick
/// does not pay a pactl round trip the menu just made.
fn refresh_missing_labels(state: &MenuState, labels: &RefCell<DeviceLabels>) {
    let known = {
        let labels = labels.borrow();
        (state.mic_device.is_empty()
            || labels
                .microphones
                .iter()
                .any(|device| device.name == state.mic_device))
            && (state.speaker_device.is_empty()
                || labels
                    .outputs
                    .iter()
                    .any(|device| device.monitor_source == state.speaker_device))
    };
    if !known {
        load_device_labels(labels, true);
    }
}

fn microphone_label(state: &MenuState, labels: &DeviceLabels) -> String {
    if !state.microphone || state.mic_device.is_empty() {
        return "No microphone".into();
    }
    labels
        .microphones
        .iter()
        .find(|device| device.name == state.mic_device)
        .map(|device| device.description.clone())
        .unwrap_or_else(|| state.mic_device.clone())
}

fn speaker_label(state: &MenuState, labels: &DeviceLabels) -> String {
    if !state.speaker || state.speaker_device.is_empty() {
        return "No system audio".into();
    }
    labels
        .outputs
        .iter()
        .find(|device| device.monitor_source == state.speaker_device)
        .map(|device| device.description.clone())
        .unwrap_or_else(|| state.speaker_device.clone())
}

/// Pulse heads every device of a sound card with the card's name ("Raptor
/// Lake-P/U/H cAVS Stereo Microphone", "... Digital Microphone"), which is all
/// a narrow cell has room for. Drop the leading words a description shares
/// with a sibling, keeping the shortest such prefix so outputs that differ
/// only at the end ("... DisplayPort 1 Output") keep their distinguishing
/// words. A device with no sibling keeps its whole description.
fn without_shared_card_name<'a>(description: &str, all: impl Iterator<Item = &'a str>) -> String {
    let words: Vec<&str> = description.split_whitespace().collect();
    let shared = all
        .filter(|other| *other != description)
        .map(|other| {
            words
                .iter()
                .zip(other.split_whitespace())
                .take_while(|(a, b)| *a == b)
                .count()
        })
        .filter(|&count| count > 0 && count < words.len())
        .min();
    match shared {
        Some(count) => words[count..].join(" "),
        None => description.to_string(),
    }
}

/// The label a device cell draws: the description without the card name its
/// siblings share and, when that still does not fit, also without trailing
/// words a sibling shares ("Output", "Microphone"). What remains is elided in
/// the middle by the caller; the menu keeps the full description.
fn short_device_label(description: &str, all: &[&str], fits: impl Fn(&str) -> bool) -> String {
    let label = without_shared_card_name(description, all.iter().copied());
    if fits(&label) {
        return label;
    }
    let words: Vec<&str> = label.split_whitespace().collect();
    let shared = all
        .iter()
        .filter(|other| **other != description)
        .map(|other| {
            words
                .iter()
                .rev()
                .zip(other.split_whitespace().rev())
                .take_while(|(a, b)| *a == b)
                .count()
        })
        .filter(|&count| count > 0 && count < words.len())
        .min();
    match shared {
        Some(count) => words[..words.len() - count].join(" "),
        None => label,
    }
}

fn label_for(index: usize, state: &MenuState, labels: &DeviceLabels) -> String {
    match index {
        5 if state.recording => microphone_label(state, labels),
        6 if state.recording => speaker_label(state, labels),
        6 if state.timer_seconds > 0 => format!("{}s", state.timer_seconds),
        _ => LABELS[index].into(),
    }
}

fn draw_menu(
    context: &cairo::Context,
    state: &MenuState,
    hovered: i32,
    rects: &[Rect; 8],
    labels: &DeviceLabels,
    levels: (f64, f64),
) {
    context.set_antialias(cairo::Antialias::Best);
    let panel = Rect {
        x: 1.5,
        y: 1.5,
        width: panel_width(rects) - 3.0,
        height: ITEM_HEIGHT + 2.0 * OUTER_PAD - 3.0,
    };
    rounded_rect(
        context,
        Rect {
            y: panel.y + 3.0,
            ..panel
        },
        PANEL_RADIUS,
    );
    context.set_source_rgba(0.0, 0.0, 0.0, 110.0 / 255.0);
    let _ = context.fill();
    rounded_rect(context, panel, PANEL_RADIUS);
    context.set_source_rgba(20.0 / 255.0, 20.0 / 255.0, 20.0 / 255.0, 250.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 42.0 / 255.0);
    context.set_line_width(1.0);
    let _ = context.stroke();

    // Shot and Video share one switch: an inset track with the selected half
    // carrying the accent.
    let shot = rects[0];
    let video = rects[1];
    let track = Rect {
        x: shot.x + 3.0,
        y: shot.y + 4.0,
        width: video.x + video.width - shot.x - 6.0,
        height: shot.height - 8.0,
    };
    let track_radius = PANEL_RADIUS * track.height / panel.height;
    rounded_rect(context, track, track_radius);
    context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
    let _ = context.fill();
    for (index, &cell) in rects.iter().enumerate().take(2) {
        let selected = (index == 1) == state.recording;
        let hovered_half = hovered == index as i32 && !selected;
        if !selected && !hovered_half {
            continue;
        }
        let thumb = Rect {
            x: cell.x + 4.5,
            y: cell.y + 6.0,
            width: cell.width - 9.0,
            height: cell.height - 12.0,
        };
        let thumb_radius = track_radius * thumb.height / track.height;
        rounded_rect(context, thumb, thumb_radius);
        if selected {
            context.set_source_rgba(
                ACCENT.0 / 255.0,
                ACCENT.1 / 255.0,
                ACCENT.2 / 255.0,
                235.0 / 255.0,
            );
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 22.0 / 255.0);
        }
        let _ = context.fill();
    }

    let pango_context = pangocairo::functions::create_context(context);
    set_font_options(&pango_context);
    for (index, &cell) in rects.iter().enumerate() {
        let disabled = item_is_disabled(index, state);
        let active = drawing_active(index, state);
        let device_on = device_on(index, state);
        let is_hovered = hovered == index as i32 && !disabled;
        if (active || is_hovered) && index > 1 {
            let highlight = Rect {
                x: cell.x + 3.0,
                y: cell.y + 3.0,
                width: cell.width - 6.0,
                height: cell.height - 6.0,
            };
            rounded_rect(context, highlight, PANEL_RADIUS - 3.0);
            if active {
                context.set_source_rgba(
                    ACCENT.0 / 255.0,
                    ACCENT.1 / 255.0,
                    ACCENT.2 / 255.0,
                    224.0 / 255.0,
                );
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, 16.0 / 255.0);
            }
            let _ = context.fill();
        }
        let icon_color = if active {
            (1.0, 1.0, 1.0, 1.0)
        } else if device_on {
            (1.0, 138.0 / 255.0, 64.0 / 255.0, 1.0)
        } else if disabled {
            (115.0 / 255.0, 115.0 / 255.0, 122.0 / 255.0, 1.0)
        } else {
            (185.0 / 255.0, 186.0 / 255.0, 193.0 / 255.0, 1.0)
        };
        let is_close = index == 7;
        // Mic and Speaker carry a device name, a slash when the input is off,
        // and a level meter while Video is selected.
        let device_slot = state.recording && (index == 5 || index == 6);
        let weight = if active || device_on {
            pango::Weight::Medium
        } else {
            pango::Weight::Normal
        };
        let mut label = if is_close {
            String::new()
        } else {
            label_for(index, state, labels)
        };
        if device_slot {
            let room = (cell.width - 2.0 * ICON_PAD - ICON_SIZE - ICON_GAP).max(0.0);
            let all: Vec<&str> = if index == 5 {
                labels
                    .microphones
                    .iter()
                    .map(|d| d.description.as_str())
                    .collect()
            } else {
                labels
                    .outputs
                    .iter()
                    .map(|d| d.description.as_str())
                    .collect()
            };
            if all.contains(&label.as_str()) {
                label = short_device_label(&label, &all, |text| {
                    logical_width(&label_layout(&pango_context, text, weight)) <= room
                });
            }
        }
        let layout = label_layout(&pango_context, &label, weight);
        let label_width = if is_close || device_slot {
            0.0
        } else {
            logical_width(&layout)
        };
        let group_width = if is_close {
            0.0
        } else {
            ICON_SIZE + ICON_GAP + label_width
        };
        let (cell_x, cell_y) = cell.center();
        // A device group sits at the cell's left edge; the rest stay centred.
        let group_left = if device_slot {
            cell.x + ICON_PAD
        } else {
            cell_x - group_width / 2.0
        };
        let icon_center = (
            if is_close {
                cell_x
            } else {
                group_left + ICON_SIZE / 2.0
            },
            if device_slot { cell_y - 2.0 } else { cell_y },
        );
        let icon_ink = if is_close && !is_hovered {
            (190.0 / 255.0, 190.0 / 255.0, 196.0 / 255.0, 1.0)
        } else {
            icon_color
        };
        draw_icon(context, glyph_for(index, state), icon_center, icon_ink);
        if device_slot && !device_on {
            context.set_source_rgba(150.0 / 255.0, 150.0 / 255.0, 158.0 / 255.0, 1.0);
            context.set_line_width(1.5);
            context.set_line_cap(cairo::LineCap::Round);
            context.move_to(icon_center.0 - 8.0, icon_center.1 - 8.0);
            context.line_to(icon_center.0 + 8.0, icon_center.1 + 8.0);
            let _ = context.stroke();
        }
        if !is_close {
            let text_color = if active || device_on {
                (1.0, 1.0, 1.0, 1.0)
            } else if disabled {
                (115.0 / 255.0, 115.0 / 255.0, 122.0 / 255.0, 1.0)
            } else if device_slot {
                (150.0 / 255.0, 150.0 / 255.0, 158.0 / 255.0, 1.0)
            } else {
                (228.0 / 255.0, 228.0 / 255.0, 232.0 / 255.0, 1.0)
            };
            let text_left = group_left + ICON_SIZE + ICON_GAP;
            if device_slot {
                // Descriptions differ late in the string, so keep both ends.
                let width = (cell.x + cell.width - ICON_PAD - text_left).max(0.0);
                layout.set_width((width * f64::from(pango::SCALE)).round() as i32);
                layout.set_ellipsize(pango::EllipsizeMode::Middle);
                layout.set_single_paragraph_mode(true);
            }
            // Centre the icon on the cap height rather than the whole line
            // box, the same optical fit the C++ toolbar draws.
            let (ink, _) = label_layout(&pango_context, "H", weight).extents();
            let cap_height = f64::from(ink.height()) / f64::from(pango::SCALE);
            let baseline = icon_center.1 + cap_height / 2.0;
            context.set_source_rgba(text_color.0, text_color.1, text_color.2, text_color.3);
            context.move_to(
                text_left,
                baseline - f64::from(layout.baseline()) / f64::from(pango::SCALE),
            );
            pangocairo::functions::show_layout(context, &layout);
            if device_slot && device_on {
                let meter = Rect {
                    x: text_left,
                    y: cell.y + cell.height - 8.0,
                    width: (cell.x + cell.width - ICON_PAD - text_left).max(0.0),
                    height: 3.0,
                };
                rounded_rect(context, meter, 1.5);
                context.set_source_rgba(1.0, 1.0, 1.0, 30.0 / 255.0);
                let _ = context.fill();
                let level = match index {
                    5 => levels.0,
                    _ => levels.1,
                }
                .clamp(0.0, 1.0);
                if level > 0.0 {
                    rounded_rect(
                        context,
                        Rect {
                            width: meter.width * level,
                            ..meter
                        },
                        1.5,
                    );
                    context.set_source_rgba(1.0, 138.0 / 255.0, 64.0 / 255.0, 1.0);
                    let _ = context.fill();
                }
            }
        }
        if separator_after(index) {
            let x = cell.x + cell.width + 7.0;
            context.set_source_rgba(1.0, 1.0, 1.0, 28.0 / 255.0);
            context.set_line_width(1.0);
            context.move_to(x, 11.0);
            context.line_to(x, PANEL_HEIGHT - 12.0);
            let _ = context.stroke();
        }
    }
}

fn install_transparent_window_css(display: &gdk::Display) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let provider = CssProvider::new();
        provider.load_from_data(
            "window.apexshot-capture-menu, window.apexshot-capture-menu > * { background: transparent; }\n\
             popover.apexshot-device-popover {\
                 background: transparent;\
                 border: none;\
                 box-shadow: none;\
                 outline: none;\
                 padding: 0;\
             }\n\
             popover.apexshot-device-popover > contents {\
                 background: #252526;\
                 border: 1px solid rgba(255,255,255,0.10);\
                 border-radius: 10px;\
                 padding: 4px;\
                 box-shadow: none;\
                 min-width: 294px;\
             }\n\
             .apexshot-device-list { background: transparent; }\n\
             button.apexshot-device-item {\
                 background: transparent;\
                 border: none;\
                 box-shadow: none;\
                 outline: none;\
                 padding: 7px 8px 7px 8px;\
                 border-radius: 6px;\
                 color: rgba(255,255,255,0.90);\
                 font-family: \"DM Sans\";\
                 font-size: 14px;\
                 font-weight: 500;\
                 min-height: 0;\
                 min-width: 0;\
             }\n\
             button.apexshot-device-item:hover { background: rgba(255,255,255,0.07); }\n\
             button.apexshot-device-item:disabled { color: rgba(255,255,255,0.24); }\n\
             button.apexshot-device-item label {\
                 color: inherit;\
                 font-family: \"DM Sans\";\
                 font-size: 14px;\
                 font-weight: 500;\
             }\n\
             .apexshot-device-separator {\
                 background: rgba(255,255,255,0.06);\
                 min-height: 1px;\
                 margin: 4px 8px;\
             }",
        );
        gtk4::style_context_add_provider_for_display(
            display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

/// One row of a Quick Capture device popover.
struct DeviceChoice {
    label: String,
    value: String,
}

/// The trailing tick, drawn with cairo so it matches the C++ menu's mark
/// without depending on a glyph being present in the installed fonts.
fn device_tick(ticked: bool) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(16);
    area.set_content_height(16);
    area.set_valign(gtk4::Align::Center);
    // 294 px row - 8 px padding each side - 16 px tick - this = 216 px of
    // label, the width the C++ menu elides at.
    area.set_margin_start(46);
    if ticked {
        area.set_draw_func(|_, context, width, height| {
            let width = f64::from(width);
            let height = f64::from(height);
            context.set_source_rgba(1.0, 1.0, 1.0, 219.0 / 255.0);
            context.set_line_width(1.8);
            context.set_line_cap(cairo::LineCap::Round);
            context.move_to(width * 0.20, height * 0.52);
            context.line_to(width * 0.42, height * 0.74);
            context.line_to(width * 0.80, height * 0.28);
            let _ = context.stroke();
        });
    }
    area
}

fn device_row(
    label: &str,
    ticked: bool,
    sensitive: bool,
    activate: impl Fn() + 'static,
) -> gtk4::Button {
    let button = gtk4::Button::new();
    button.add_css_class("apexshot-device-item");
    button.set_has_frame(false);
    button.set_sensitive(sensitive);
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    let text = gtk4::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    // Pulse names a card's outputs alike up front, so keep both ends. A
    // one-char natural width lets the popover's fixed width set the label's.
    text.set_ellipsize(pango::EllipsizeMode::Middle);
    text.set_max_width_chars(1);
    row.append(&text);
    row.append(&device_tick(ticked));
    button.set_child(Some(&row));
    button.connect_clicked(move |_| activate());
    button
}

/// A dark list anchored under a toolbar item, styled like the C++ menu.
#[allow(clippy::too_many_arguments)]
fn show_device_popover(
    drawing: &DrawingArea,
    rect: Rect,
    empty_label: &str,
    off_label: &str,
    devices: Vec<DeviceChoice>,
    selected: Option<&str>,
    off_selected: bool,
    on_choice: impl Fn(Option<String>) + 'static,
) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    popover.add_css_class("apexshot-device-popover");
    popover.set_parent(drawing);
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    // Left edge on the item and 6 px below the panel, where the C++ menu opens.
    popover.set_halign(gtk4::Align::Start);
    popover.set_offset(0, 6);
    popover.set_autohide(true);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        rect.x.round() as i32,
        PANEL_HEIGHT.round() as i32 - 1,
        rect.width.round() as i32,
        1,
    )));

    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    list.add_css_class("apexshot-device-list");
    let on_choice = Rc::new(on_choice);
    if devices.is_empty() {
        list.append(&device_row(empty_label, false, false, || {}));
    } else {
        for device in &devices {
            // An input that is off ticks only its "Don't record" row.
            let ticked = !off_selected && selected == Some(device.value.as_str());
            let value = device.value.clone();
            // Weak: the row owns the closure, the popover owns the row.
            let popover = popover.downgrade();
            let on_choice = on_choice.clone();
            list.append(&device_row(&device.label, ticked, true, move || {
                (*on_choice)(Some(value.clone()));
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
            }));
        }
    }
    let separator = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    separator.add_css_class("apexshot-device-separator");
    list.append(&separator);
    {
        let popover = popover.downgrade();
        let on_choice = on_choice.clone();
        list.append(&device_row(off_label, off_selected, true, move || {
            (*on_choice)(None);
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }));
    }

    popover.set_child(Some(&list));
    let drawing = drawing.clone();
    popover.connect_closed(move |popover| {
        popover.unparent();
        let _ = drawing.grab_focus();
    });
    popover
}

/// Recomputes the cells for the current mode and gives the surface its new
/// width. The layer surface is anchored top only, so the compositor keeps it
/// centred on the monitor.
fn relayout(
    state: &RefCell<MenuState>,
    rects: &RefCell<[Rect; 8]>,
    monitor_width: Option<f64>,
    drawing: &DrawingArea,
    window: &Window,
) {
    let next = item_rects(&item_widths(&state.borrow(), monitor_width));
    let width = panel_width(&next).round() as i32;
    let height = PANEL_HEIGHT.round() as i32;
    *rects.borrow_mut() = next;
    drawing.set_content_width(width);
    drawing.set_content_height(height);
    drawing.set_size_request(width, height);
    window.set_default_size(width, height);
    drawing.queue_resize();
    drawing.queue_draw();
}

/// Points the daemon's meters at the devices the cells name and polls them
/// while Video shows a live input. The source removes itself when Video or
/// both inputs go away.
fn update_meter_poll(
    state: &Rc<RefCell<MenuState>>,
    levels: &Rc<Cell<(f64, f64)>>,
    drawing: &DrawingArea,
    poll: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    let (mic, speaker) = {
        let state = state.borrow();
        let source = |enabled: bool, device: &str| {
            (enabled && !device.is_empty()).then(|| device.to_string())
        };
        (
            source(state.recording && state.microphone, &state.mic_device),
            source(state.recording && state.speaker, &state.speaker_device),
        )
    };
    if mic.is_none() && speaker.is_none() {
        if let Some(source) = poll.borrow_mut().take() {
            source.remove();
        }
        levels.set((0.0, 0.0));
        crate::daemon::set_meter_sources(None, None);
        drawing.queue_draw();
        return;
    }
    crate::daemon::set_meter_sources(mic, speaker);
    if poll.borrow().is_some() {
        return;
    }
    let state = state.clone();
    let levels = levels.clone();
    let drawing = drawing.clone();
    let poll_slot = poll.clone();
    let source = glib::timeout_add_local(Duration::from_millis(33), move || {
        let (mic, speaker) = {
            let state = state.borrow();
            (
                state.recording && state.microphone,
                state.recording && state.speaker,
            )
        };
        if !mic && !speaker {
            // The id leaves its slot without removing this source; returning
            // Break is what stops it.
            poll_slot.borrow_mut().take();
            levels.set((0.0, 0.0));
            return glib::ControlFlow::Break;
        }
        let (old_mic, old_speaker) = levels.get();
        let next_mic = if mic {
            let level = crate::daemon::mic_meter_level();
            if level > old_mic {
                level
            } else {
                old_mic * 0.6 + level * 0.4
            }
        } else {
            0.0
        };
        let next_speaker = if speaker {
            let level = crate::daemon::speaker_meter_level();
            if level > old_speaker {
                level
            } else {
                old_speaker * 0.6 + level * 0.4
            }
        } else {
            0.0
        };
        levels.set((next_mic, next_speaker));
        drawing.queue_draw();
        glib::ControlFlow::Continue
    });
    *poll.borrow_mut() = Some(source);
}

/// Shows a pixel-controlled port of the C++ capture toolbar on the selected display.
pub fn choose_capture_mode(target: &MonitorChoice) -> CaptureMenuResult {
    choose_capture_mode_internal(target, None, false).unwrap_or_default()
}

pub(crate) fn choose_capture_mode_on_layer_shell(
    target: &MonitorChoice,
    control_receiver: std::sync::mpsc::Receiver<crate::capture_overlay::CaptureMenuControlRequest>,
) -> Option<CaptureMenuResult> {
    choose_capture_mode_internal(target, Some(control_receiver), true)
}

fn choose_capture_mode_internal(
    target: &MonitorChoice,
    control_receiver: Option<
        std::sync::mpsc::Receiver<crate::capture_overlay::CaptureMenuControlRequest>,
    >,
    require_layer_shell: bool,
) -> Option<CaptureMenuResult> {
    let display = gdk::Display::default()?;
    let layer_shell_monitor = if gtk4_layer_shell::is_supported() {
        find_monitor_at(&display, target.x, target.y)
    } else {
        None
    };
    if require_layer_shell && layer_shell_monitor.is_none() {
        return None;
    }
    install_transparent_window_css(&display);
    // The device cells take their width from the monitor the bar sits on.
    let monitor_width = layer_shell_monitor
        .as_ref()
        .map(|monitor| f64::from(monitor.geometry().width()));
    let app_config = crate::config::load_config();
    let state = Rc::new(RefCell::new(MenuState {
        recording: false,
        ocr: false,
        timer_seconds: 0,
        microphone: false,
        speaker: false,
        mic_device: app_config.rec_mic_device,
        speaker_device: app_config.rec_speaker_device,
    }));
    let labels: Rc<RefCell<DeviceLabels>> = Rc::new(RefCell::new(DeviceLabels::default()));
    let levels = Rc::new(Cell::new((0.0, 0.0)));
    let meter_poll: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let initial_rects = item_rects(&item_widths(&state.borrow(), monitor_width));
    let rects: Rc<RefCell<[Rect; 8]>> = Rc::new(RefCell::new(initial_rects));
    let window_width = panel_width(&initial_rects).round() as i32;
    let window_height = PANEL_HEIGHT.round() as i32;
    let result = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);
    let window = Window::builder()
        .title("ApexShot Capture")
        .decorated(false)
        .resizable(false)
        .css_classes(["apexshot-capture-menu"])
        .build();
    window.set_default_size(window_width, window_height);
    if let Some(monitor) = layer_shell_monitor {
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_margin(Edge::Top, 28);
        window.set_monitor(Some(&monitor));
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_namespace(Some("apexshot-capture-menu"));
    }
    let open_popover: Rc<RefCell<Option<gtk4::Popover>>> = Rc::new(RefCell::new(None));
    let hovered = Rc::new(Cell::new(-1));
    let drawing = DrawingArea::new();
    drawing.set_content_width(window_width);
    drawing.set_content_height(window_height);
    drawing.set_size_request(window_width, window_height);
    drawing.set_focusable(true);
    {
        let state = state.clone();
        let hovered = hovered.clone();
        let rects = rects.clone();
        let labels = labels.clone();
        let levels = levels.clone();
        drawing.set_draw_func(move |_, context, _, _| {
            draw_menu(
                context,
                &state.borrow(),
                hovered.get(),
                &rects.borrow(),
                &labels.borrow(),
                levels.get(),
            )
        });
    }
    let motion = EventControllerMotion::new();
    {
        let state = state.clone();
        let hovered = hovered.clone();
        let drawing = drawing.clone();
        let rects = rects.clone();
        motion.connect_motion(move |_, x, y| {
            let menu = state.borrow();
            let next = item_at(x, y, &rects.borrow())
                .filter(|&i| !item_is_disabled(i, &menu))
                .map(|i| i as i32)
                .unwrap_or(-1);
            if hovered.replace(next) != next {
                drawing.queue_draw();
            }
        });
    }
    {
        let hovered = hovered.clone();
        let drawing = drawing.clone();
        motion.connect_leave(move |_| {
            if hovered.replace(-1) != -1 {
                drawing.queue_draw();
            }
        });
    }
    drawing.add_controller(motion);
    let click = GestureClick::builder().button(1).build();
    {
        let state = state.clone();
        let result = result.clone();
        let main_loop = main_loop.clone();
        let window = window.clone();
        let drawing = drawing.clone();
        let rects = rects.clone();
        let labels = labels.clone();
        let levels = levels.clone();
        let meter_poll = meter_poll.clone();
        click.connect_pressed(move |_, _, x, y| {
            let Some(index) = item_at(x, y, &rects.borrow()) else {
                return;
            };
            let mut menu = state.borrow_mut();
            if item_is_disabled(index, &menu) {
                return;
            }
            match index {
                0 => set_recording_mode(&mut menu, false),
                1 => set_recording_mode(&mut menu, true),
                2 => {
                    *result.borrow_mut() = Some(menu.clone().result(CaptureMenuAction::Display));
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                3 => {
                    *result.borrow_mut() = Some(menu.clone().result(CaptureMenuAction::Window));
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                4 => {
                    *result.borrow_mut() = Some(menu.clone().result(CaptureMenuAction::Area));
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                5 if menu.recording => menu.microphone = !menu.microphone,
                5 => menu.ocr = !menu.ocr,
                6 if menu.recording => menu.speaker = !menu.speaker,
                6 => {
                    menu.timer_seconds = match menu.timer_seconds {
                        0 => 3,
                        3 => 5,
                        5 => 10,
                        _ => 0,
                    }
                }
                7 => {
                    *result.borrow_mut() = Some(CaptureMenuResult::default());
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                _ => {}
            }
            drop(menu);
            if matches!(index, 0 | 1) {
                // The mode decides whether the device cells exist and how
                // wide they are, so the surface grows or shrinks with it.
                if state.borrow().recording {
                    load_device_labels(&labels, false);
                }
                relayout(&state, &rects, monitor_width, &drawing, &window);
            }
            update_meter_poll(&state, &levels, &drawing, &meter_poll);
            drawing.queue_draw();
        });
    }
    drawing.add_controller(click);
    // Right-click while Video is selected opens the same device menu the
    // Windows toolbar draws under Mic and Speaker.
    let device_click = GestureClick::builder().button(3).build();
    {
        let state = state.clone();
        let drawing = drawing.clone();
        let rects = rects.clone();
        let labels = labels.clone();
        let levels = levels.clone();
        let meter_poll = meter_poll.clone();
        let open_popover = open_popover.clone();
        device_click.connect_pressed(move |_, _, x, y| {
            let Some(index) = item_at(x, y, &rects.borrow()) else {
                return;
            };
            if !matches!(index, 5 | 6) || !state.borrow().recording {
                return;
            }
            // One device menu at a time; the closed handler clears this slot.
            let existing = open_popover.borrow().clone();
            if let Some(existing) = existing {
                existing.popdown();
            }
            let rect = rects.borrow()[index];
            let popover = if index == 5 {
                let devices = crate::recording::list_audio_inputs();
                let enabled = state.borrow().microphone;
                let current = state.borrow().mic_device.clone();
                let selected = if enabled && devices.iter().any(|device| device.name == current) {
                    Some(current.clone())
                } else {
                    devices
                        .iter()
                        .find(|device| device.default)
                        .map(|device| device.name.clone())
                };
                // A saved microphone that disappeared silently falls back to
                // the default, so the request carries a source that exists.
                if let (true, Some(selected)) = (enabled, selected.as_ref()) {
                    if state.borrow().mic_device != *selected {
                        state.borrow_mut().mic_device = selected.to_string();
                    }
                }
                let choices = devices
                    .into_iter()
                    .map(|device| DeviceChoice {
                        label: device.description,
                        value: device.name,
                    })
                    .collect();
                let state = state.clone();
                let redraw = drawing.clone();
                let labels = labels.clone();
                let levels = levels.clone();
                let meter_poll = meter_poll.clone();
                show_device_popover(
                    &drawing,
                    rect,
                    "No microphone detected",
                    "Don't record microphone",
                    choices,
                    selected.as_deref(),
                    !enabled,
                    move |value| {
                        let mut menu = state.borrow_mut();
                        match value {
                            Some(name) => {
                                menu.microphone = true;
                                menu.mic_device = name;
                            }
                            None => menu.microphone = false,
                        }
                        drop(menu);
                        refresh_missing_labels(&state.borrow(), &labels);
                        update_meter_poll(&state, &levels, &redraw, &meter_poll);
                        redraw.queue_draw();
                    },
                )
            } else {
                let devices = crate::recording::list_audio_outputs();
                let enabled = state.borrow().speaker;
                let current = state.borrow().speaker_device.clone();
                let selected = if enabled
                    && devices
                        .iter()
                        .any(|device| device.monitor_source == current)
                {
                    Some(current.clone())
                } else {
                    devices
                        .iter()
                        .find(|device| device.default)
                        .map(|device| device.monitor_source.clone())
                };
                if let (true, Some(selected)) = (enabled, selected.as_ref()) {
                    if state.borrow().speaker_device != *selected {
                        state.borrow_mut().speaker_device = selected.to_string();
                    }
                }
                let choices = devices
                    .into_iter()
                    .map(|device| DeviceChoice {
                        label: device.description,
                        value: device.monitor_source,
                    })
                    .collect();
                let state = state.clone();
                let redraw = drawing.clone();
                let labels = labels.clone();
                let levels = levels.clone();
                let meter_poll = meter_poll.clone();
                show_device_popover(
                    &drawing,
                    rect,
                    "No audio output detected",
                    "Don't record system audio",
                    choices,
                    selected.as_deref(),
                    !enabled,
                    move |value| {
                        let mut menu = state.borrow_mut();
                        match value {
                            Some(monitor) => {
                                menu.speaker = true;
                                menu.speaker_device = monitor;
                            }
                            None => menu.speaker = false,
                        }
                        drop(menu);
                        refresh_missing_labels(&state.borrow(), &labels);
                        update_meter_poll(&state, &levels, &redraw, &meter_poll);
                        redraw.queue_draw();
                    },
                )
            };
            open_popover.replace(Some(popover.clone()));
            let open_popover = open_popover.clone();
            popover.connect_closed(move |_| {
                open_popover.replace(None);
            });
            popover.popup();
        });
    }
    drawing.add_controller(device_click);
    window.set_child(Some(&drawing));
    let key = EventControllerKey::new();
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        let window = window.clone();
        let open_popover = open_popover.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                // Escape closes an open device menu first; it must not cancel
                // the toolbar underneath it.
                let open_device_menu = open_popover.borrow().clone();
                if let Some(popover) = open_device_menu {
                    popover.popdown();
                    return glib::Propagation::Stop;
                }
                *result.borrow_mut() = Some(CaptureMenuResult::default());
                window.close();
                main_loop.quit();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    window.add_controller(key);
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        window.connect_close_request(move |_| {
            if result.borrow().is_none() {
                *result.borrow_mut() = Some(CaptureMenuResult::default());
            }
            main_loop.quit();
            glib::Propagation::Proceed
        });
    }
    let control_poll = control_receiver.map(|receiver| {
        let result = result.clone();
        let main_loop = main_loop.clone();
        let window = window.clone();
        let drawing = drawing.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            loop {
                match receiver.try_recv() {
                    Ok(crate::capture_overlay::CaptureMenuControlRequest::Focus) => {
                        window.present();
                        let _ = drawing.grab_focus();
                    }
                    Ok(crate::capture_overlay::CaptureMenuControlRequest::Cancel) => {
                        *result.borrow_mut() = Some(CaptureMenuResult::default());
                        window.close();
                        main_loop.quit();
                        return glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
            glib::ControlFlow::Continue
        })
    });
    window.present();
    let _ = drawing.grab_focus();
    main_loop.run();
    if let Some(control_poll) = control_poll {
        control_poll.remove();
    }
    if let Some(source) = meter_poll.borrow_mut().take() {
        source.remove();
    }
    // Release this process's meter streams; a capture starts right after and
    // must not share the device with a stream the idle reaper has not reached.
    crate::daemon::stop_meter_streams();
    window.hide();
    window.destroy();
    while glib::MainContext::default().iteration(false) {}
    let selected = result.borrow_mut().take().unwrap_or_default();
    Some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_state_matches_the_qt_toolbar_constraints() {
        let shot = MenuState::default();
        assert!(item_is_active(0, &shot));
        assert!(!item_is_disabled(2, &shot));
        let timer = MenuState {
            timer_seconds: 3,
            ..shot.clone()
        };
        assert!(item_is_disabled(3, &timer));
        assert!(item_is_disabled(4, &timer));
        let video = MenuState {
            recording: true,
            ..shot
        };
        assert!(item_is_active(1, &video));
        assert!(!item_is_disabled(3, &video));
        assert!(item_is_disabled(4, &video));
    }

    #[test]
    fn shot_ocr_disables_display_and_timer_but_allows_area() {
        let shot = MenuState {
            ocr: true,
            ..MenuState::default()
        };

        assert!(item_is_disabled(2, &shot));
        assert!(!item_is_disabled(4, &shot));
        assert!(item_is_disabled(6, &shot));
    }

    #[test]
    fn video_audio_toggles_survive_shot_selection_for_area_capture() {
        let shot = MenuState::default();
        let mut video = MenuState {
            recording: true,
            microphone: true,
            speaker: true,
            mic_device: "alsa_input.usb-mic".into(),
            speaker_device: "alsa_output.speaker.monitor".into(),
            ..shot
        };

        set_recording_mode(&mut video, false);

        assert!(!video.recording);
        assert!(video.microphone);
        assert!(video.speaker);
        assert!(!item_is_disabled(4, &video));
        assert_eq!(
            video.result(CaptureMenuAction::Area),
            CaptureMenuResult {
                action: CaptureMenuAction::Area,
                recording: false,
                ocr: false,
                timer_seconds: 0,
                microphone: true,
                speaker: true,
                mic_device: "alsa_input.usb-mic".into(),
                speaker_device: "alsa_output.speaker.monitor".into(),
            }
        );
    }

    #[test]
    fn item_rects_match_the_cpp_toolbar_layout() {
        let widths = [60.0, 62.0, 82.0, 86.0, 66.0, 70.0, 82.0, 44.0];
        let rects = item_rects(&widths);
        assert_eq!(
            (rects[0].x, rects[0].y, rects[0].width, rects[0].height),
            (6.0, 6.0, 60.0, 44.0)
        );
        assert_eq!(rects[1].x, 66.0);
        assert_eq!(rects[2].x, 140.0);
        assert_eq!(rects[5].x, 386.0);
        assert_eq!(rects[7].x, 550.0);
        assert_eq!(panel_width(&rects), 600.0);
    }

    #[test]
    fn generated_icons_are_usable_paths() {
        use crate::overlay::capture_menu_icons::{
            Seg, APP_WINDOW, CAMERA, MIC, MONITOR, SCAN, SCAN_TEXT, TIMER, VIDEO, VOLUME_2, X,
        };

        for glyph in [
            CAMERA, VIDEO, MONITOR, APP_WINDOW, SCAN, SCAN_TEXT, TIMER, MIC, VOLUME_2, X,
        ] {
            assert!(!glyph.segs.is_empty());
            assert!(matches!(glyph.segs[0], Seg::Move(..)));
            let (min_x, min_y, max_x, max_y) = glyph.bounds;
            assert!(max_x > min_x);
            assert!(max_y > min_y);
        }
    }

    #[test]
    fn cell_labels_drop_the_shared_card_name() {
        let mics = [
            "Raptor Lake-P/U/H cAVS Stereo Microphone",
            "Raptor Lake-P/U/H cAVS Digital Microphone",
            "Easy Effects Source",
        ];
        assert_eq!(
            without_shared_card_name(mics[1], mics.iter().copied()),
            "Digital Microphone"
        );
        assert_eq!(
            without_shared_card_name(mics[2], mics.iter().copied()),
            "Easy Effects Source"
        );
        let outputs = [
            "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 3 Output",
            "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 2 Output",
            "Raptor Lake-P/U/H cAVS Speaker",
        ];
        assert_eq!(
            without_shared_card_name(outputs[0], outputs.iter().copied()),
            "HDMI / DisplayPort 3 Output"
        );
        assert_eq!(
            without_shared_card_name("USB Headset", ["USB Headset"].into_iter()),
            "USB Headset"
        );
    }

    #[test]
    fn narrow_cells_also_drop_shared_trailing_words() {
        let outputs = [
            "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 3 Output",
            "Raptor Lake-P/U/H cAVS HDMI / DisplayPort 2 Output",
            "Raptor Lake-P/U/H cAVS Speaker",
        ];
        assert_eq!(
            short_device_label(outputs[0], &outputs, |_| true),
            "HDMI / DisplayPort 3 Output"
        );
        assert_eq!(
            short_device_label(outputs[0], &outputs, |_| false),
            "HDMI / DisplayPort 3"
        );
        let mics = [
            "Raptor Lake-P/U/H cAVS Stereo Microphone",
            "Raptor Lake-P/U/H cAVS Digital Microphone",
        ];
        assert_eq!(short_device_label(mics[1], &mics, |_| false), "Digital");
        assert_eq!(
            short_device_label("Easy Effects Source", &["Easy Effects Source"], |_| false),
            "Easy Effects Source"
        );
    }

    #[test]
    fn device_toggles_only_count_while_recording() {
        let shot = MenuState {
            microphone: true,
            speaker: true,
            ..MenuState::default()
        };
        assert!(!device_on(5, &shot));
        assert!(!device_on(6, &shot));

        let video = MenuState {
            recording: true,
            ..shot
        };
        assert!(device_on(5, &video));
        assert!(device_on(6, &video));
    }

    #[test]
    fn device_cells_split_the_monitor_width_like_the_cpp_toolbar() {
        let widths = [60.0, 62.0, 82.0, 86.0, 66.0, 70.0, 82.0, 44.0];
        // Three separators and the outer pads stay fixed; the two device
        // cells share whatever the monitor leaves.
        let fixed = 2.0 * OUTER_PAD
            + 3.0 * SEPARATOR
            + widths[0]
            + widths[1]
            + widths[2]
            + widths[3]
            + widths[4]
            + widths[7];
        assert_eq!(
            device_cell_width(&widths, Some(700.0)),
            (700.0 - 24.0 - fixed) / 2.0
        );
        assert_eq!(device_cell_width(&widths, Some(1400.0)), 160.0);
        assert_eq!(device_cell_width(&widths, Some(300.0)), 74.0);
        assert_eq!(device_cell_width(&widths, None), 160.0);
    }

    #[test]
    fn device_cells_show_the_chosen_description() {
        let labels = DeviceLabels {
            loaded: true,
            microphones: vec![crate::recording::AudioInputDevice {
                name: "alsa_input.builtin".into(),
                description: "Built-in Microphone".into(),
                default: true,
            }],
            outputs: vec![crate::recording::AudioOutputDevice {
                name: "alsa_output.speakers".into(),
                description: "Speakers".into(),
                monitor_source: "alsa_output.speakers.monitor".into(),
                default: true,
            }],
        };
        let video = MenuState {
            recording: true,
            microphone: true,
            speaker: true,
            mic_device: "alsa_input.builtin".into(),
            speaker_device: "alsa_output.speakers.monitor".into(),
            ..MenuState::default()
        };
        assert_eq!(label_for(5, &video, &labels), "Built-in Microphone");
        assert_eq!(label_for(6, &video, &labels), "Speakers");

        let off = MenuState {
            microphone: false,
            speaker: false,
            ..video.clone()
        };
        assert_eq!(label_for(5, &off, &labels), "No microphone");
        assert_eq!(label_for(6, &off, &labels), "No system audio");

        // A source the listing missed still names itself rather than going
        // blank, the way the C++ label falls back to the name.
        let missing = MenuState {
            mic_device: "alsa_input.usb".into(),
            ..video
        };
        assert_eq!(label_for(5, &missing, &labels), "alsa_input.usb");
    }
}
