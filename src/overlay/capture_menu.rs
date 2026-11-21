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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMenuResult {
    pub action: CaptureMenuAction,
    pub recording: bool,
    pub ocr: bool,
    pub timer_seconds: u8,
    pub microphone: bool,
    pub speaker: bool,
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
        }
    }
}

#[derive(Clone, Copy)]
struct MenuState {
    recording: bool,
    ocr: bool,
    timer_seconds: u8,
    microphone: bool,
    speaker: bool,
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

fn item_widths() -> [f64; 8] {
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

fn item_is_disabled(index: usize, state: MenuState) -> bool {
    (state.recording && index == 4)
        || (!state.recording && index == 2 && state.ocr)
        || (!state.recording && index == 3 && (state.timer_seconds > 0 || state.ocr))
        || (!state.recording && index == 4 && state.timer_seconds > 0)
        || (!state.recording && index == 5 && state.timer_seconds > 0)
        || (!state.recording && index == 6 && state.ocr)
}

fn item_is_active(index: usize, state: MenuState) -> bool {
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
fn drawing_active(index: usize, state: MenuState) -> bool {
    item_is_active(index, state) && !device_on(index, state)
}

fn device_on(index: usize, state: MenuState) -> bool {
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

fn glyph_for(index: usize, state: MenuState) -> &'static icons::Glyph {
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

fn label_for(index: usize, state: MenuState) -> String {
    match index {
        5 if state.recording => "Mic".into(),
        6 if state.recording => "Speaker".into(),
        6 if state.timer_seconds > 0 => format!("{}s", state.timer_seconds),
        _ => LABELS[index].into(),
    }
}

fn draw_menu(context: &cairo::Context, state: MenuState, hovered: i32, rects: &[Rect; 8]) {
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
        let label = if is_close {
            String::new()
        } else {
            label_for(index, state)
        };
        let weight = if active || device_on {
            pango::Weight::Medium
        } else {
            pango::Weight::Normal
        };
        let layout = label_layout(&pango_context, &label, weight);
        let label_width = if is_close {
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
        let group_left = cell_x - group_width / 2.0;
        let icon_center = (
            if is_close {
                cell_x
            } else {
                group_left + ICON_SIZE / 2.0
            },
            cell_y,
        );
        let icon_ink = if is_close && !is_hovered {
            (190.0 / 255.0, 190.0 / 255.0, 196.0 / 255.0, 1.0)
        } else {
            icon_color
        };
        draw_icon(context, glyph_for(index, state), icon_center, icon_ink);
        if !is_close {
            let text_color = if active || device_on {
                (1.0, 1.0, 1.0, 1.0)
            } else if disabled {
                (115.0 / 255.0, 115.0 / 255.0, 122.0 / 255.0, 1.0)
            } else {
                (228.0 / 255.0, 228.0 / 255.0, 232.0 / 255.0, 1.0)
            };
            // Centre the icon on the cap height rather than the whole line
            // box, the same optical fit the C++ toolbar draws.
            let (ink, _) = label_layout(&pango_context, "H", weight).extents();
            let cap_height = f64::from(ink.height()) / f64::from(pango::SCALE);
            let baseline = icon_center.1 + cap_height / 2.0;
            let text_left = group_left + ICON_SIZE + ICON_GAP;
            context.set_source_rgba(text_color.0, text_color.1, text_color.2, text_color.3);
            context.move_to(
                text_left,
                baseline - f64::from(layout.baseline()) / f64::from(pango::SCALE),
            );
            pangocairo::functions::show_layout(context, &layout);
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
        provider.load_from_data("window.apexshot-capture-menu, window.apexshot-capture-menu > * { background: transparent; }");
        gtk4::style_context_add_provider_for_display(display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    });
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
    let widths = item_widths();
    let rects = Rc::new(item_rects(&widths));
    let window_width = panel_width(&rects).round() as i32;
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
    let state = Rc::new(RefCell::new(MenuState {
        recording: false,
        ocr: false,
        timer_seconds: 0,
        microphone: false,
        speaker: false,
    }));
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
        drawing.set_draw_func(move |_, context, _, _| {
            draw_menu(context, *state.borrow(), hovered.get(), &rects)
        });
    }
    let motion = EventControllerMotion::new();
    {
        let state = state.clone();
        let hovered = hovered.clone();
        let drawing = drawing.clone();
        let rects = rects.clone();
        motion.connect_motion(move |_, x, y| {
            let menu = *state.borrow();
            let next = item_at(x, y, &rects)
                .filter(|&i| !item_is_disabled(i, menu))
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
        click.connect_pressed(move |_, _, x, y| {
            let Some(index) = item_at(x, y, &rects) else {
                return;
            };
            let mut menu = state.borrow_mut();
            if item_is_disabled(index, *menu) {
                return;
            }
            match index {
                0 => set_recording_mode(&mut menu, false),
                1 => set_recording_mode(&mut menu, true),
                2 => {
                    *result.borrow_mut() = Some(menu.result(CaptureMenuAction::Display));
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                3 => {
                    *result.borrow_mut() = Some(menu.result(CaptureMenuAction::Window));
                    drop(menu);
                    window.close();
                    main_loop.quit();
                    return;
                }
                4 => {
                    *result.borrow_mut() = Some(menu.result(CaptureMenuAction::Area));
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
            drawing.queue_draw();
        });
    }
    drawing.add_controller(click);
    window.set_child(Some(&drawing));
    let key = EventControllerKey::new();
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        let window = window.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
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
    window.hide();
    window.destroy();
    while glib::MainContext::default().iteration(false) {}
    let selected = result.borrow().unwrap_or_default();
    Some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_state_matches_the_qt_toolbar_constraints() {
        let shot = MenuState {
            recording: false,
            ocr: false,
            timer_seconds: 0,
            microphone: false,
            speaker: false,
        };
        assert!(item_is_active(0, shot));
        assert!(!item_is_disabled(2, shot));
        let timer = MenuState {
            timer_seconds: 3,
            ..shot
        };
        assert!(item_is_disabled(3, timer));
        assert!(item_is_disabled(4, timer));
        let video = MenuState {
            recording: true,
            ..shot
        };
        assert!(item_is_active(1, video));
        assert!(!item_is_disabled(3, video));
        assert!(item_is_disabled(4, video));
    }

    #[test]
    fn shot_ocr_disables_display_and_timer_but_allows_area() {
        let shot = MenuState {
            recording: false,
            ocr: true,
            timer_seconds: 0,
            microphone: false,
            speaker: false,
        };

        assert!(item_is_disabled(2, shot));
        assert!(!item_is_disabled(4, shot));
        assert!(item_is_disabled(6, shot));
    }

    #[test]
    fn video_audio_toggles_survive_shot_selection_for_area_capture() {
        let shot = MenuState {
            recording: false,
            ocr: false,
            timer_seconds: 0,
            microphone: false,
            speaker: false,
        };
        let mut video = MenuState {
            recording: true,
            microphone: true,
            speaker: true,
            ..shot
        };

        set_recording_mode(&mut video, false);

        assert!(!video.recording);
        assert!(video.microphone);
        assert!(video.speaker);
        assert!(!item_is_disabled(4, video));
        assert_eq!(
            video.result(CaptureMenuAction::Area),
            CaptureMenuResult {
                action: CaptureMenuAction::Area,
                recording: false,
                ocr: false,
                timer_seconds: 0,
                microphone: true,
                speaker: true,
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
    fn device_toggles_only_count_while_recording() {
        let shot = MenuState {
            recording: false,
            ocr: false,
            timer_seconds: 0,
            microphone: true,
            speaker: true,
        };
        assert!(!device_on(5, shot));
        assert!(!device_on(6, shot));

        let video = MenuState {
            recording: true,
            ..shot
        };
        assert!(device_on(5, video));
        assert!(device_on(6, video));
    }
}
