//! Multi-monitor display picker for the Rust GTK overlay.
//!
//! Mirrors the C++ `MonitorPicker` flow:
//! - 1 monitor → that monitor immediately (no UI)
//! - multi → floating "Choose a display" panel
//! - Esc / close button → cancelled
//! - number keys 1–9 select by sorted left-to-right order
//!
//! The picker is metadata-only: freeze/capture happens after dismiss so the
//! panel never appears in the frozen background.

use super::api::SelectionError;
use gtk4::{
    cairo, gdk, glib, prelude::*, CssProvider, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureClick, Window,
};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use crate::i18n::tfmt;

/// Metadata about a connected monitor (for logging / capture targeting).
#[derive(Debug, Clone)]
pub struct MonitorChoice {
    pub index: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub connector: String,
    pub is_primary: bool,
}

impl MonitorChoice {
    pub fn from_monitor(index: u32, monitor: &gdk::Monitor, is_primary: bool) -> Self {
        let geometry = monitor.geometry();
        let connector = monitor
            .connector()
            .map(|s| s.to_string())
            .or_else(|| monitor.model().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                let number = (index + 1).to_string();
                tfmt("Display {number}", &[("number", &number)])
            });
        Self {
            index,
            x: geometry.x(),
            y: geometry.y(),
            width: geometry.width(),
            height: geometry.height(),
            connector,
            is_primary,
        }
    }
}

/// Collect every `gdk::Monitor` currently attached to `display`.
pub fn list_monitors(display: &gdk::Display) -> Vec<gdk::Monitor> {
    let model = display.monitors();
    let n = model.n_items();
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        if let Some(obj) = model.item(i) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                out.push(monitor);
            }
        }
    }
    out
}

/// Sort monitor indices left-to-right (then top-to-bottom) for stable numbering.
fn sorted_monitor_indices(monitors: &[gdk::Monitor]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..monitors.len()).collect();
    indices.sort_by(|&a, &b| {
        let ga = monitors[a].geometry();
        let gb = monitors[b].geometry();
        ga.x()
            .cmp(&gb.x())
            .then_with(|| ga.y().cmp(&gb.y()))
            .then_with(|| a.cmp(&b))
    });
    indices
}

/// Best-effort "primary" flag: monitor whose origin is closest to (0, 0).
fn is_primary_guess(monitors: &[gdk::Monitor], index: usize) -> bool {
    let Some(best) = monitors
        .iter()
        .enumerate()
        .min_by_key(|(_, m)| {
            let g = m.geometry();
            (g.x().unsigned_abs() + g.y().unsigned_abs(), g.x(), g.y())
        })
        .map(|(i, _)| i)
    else {
        return false;
    };
    best == index
}

/// Resolve the target monitor for area capture (C++ `selectTargetScreen`).
///
/// - 0 monitors → error
/// - 1 monitor → that monitor (no UI)
/// - multi → interactive floating picker
///
/// Cancelled when the user presses Esc / clicks the close button / closes the
/// panel.
pub fn select_target_monitor() -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    let display = gdk::Display::default()
        .ok_or_else(|| SelectionError::InitError("No display found for monitor picker".into()))?;

    let monitors = list_monitors(&display);
    if monitors.is_empty() {
        return Err(SelectionError::InitError("No monitor found".into()));
    }

    if monitors.len() == 1 {
        let choice = MonitorChoice::from_monitor(0, &monitors[0], true);
        eprintln!(
            "[monitor-picker] single display — skipping picker ({} {}×{}+{}+{})",
            choice.connector, choice.width, choice.height, choice.x, choice.y
        );
        return Ok((monitors[0].clone(), choice));
    }

    select_monitor_interactive(&display, &monitors)
}

#[derive(Clone)]
struct PickerCard {
    monitor_index: i32,
    display_number: usize,
    choice: MonitorChoice,
}

// The picker sits on the same sheet as the onboarding and update windows
// (`07-pages-about-onboarding.css`, `update_window/update.css`): a borderless
// #191919 surface, a centred headline over quiet copy, and #222222 cards.
const PICKER_PANEL_RADIUS: f64 = 12.0;
const PICKER_INSET: f64 = 36.0;
const PICKER_CLOSE_SIZE: f64 = 28.0;
const PICKER_CLOSE_MARGIN: f64 = 12.0;
const PICKER_HEADLINE_H: f64 = 34.0;
const PICKER_COPY_GAP: f64 = 6.0;
const PICKER_COPY_H: f64 = 20.0;
const PICKER_CARDS_GAP: f64 = 24.0;
const PICKER_CARD_W: f64 = 220.0;
const PICKER_CARD_H: f64 = 182.0;
const PICKER_CARD_GAP: f64 = 12.0;
const PICKER_CARD_RADIUS: f64 = 9.0;
const PICKER_CARD_PAD: f64 = 14.0;
const PICKER_PREVIEW_W: f64 = 192.0;
const PICKER_PREVIEW_H: f64 = 108.0;
const PICKER_PREVIEW_RADIUS: f64 = 6.0;
const PICKER_PREVIEW_GAP: f64 = 10.0;
const PICKER_NAME_H: f64 = 18.0;
const PICKER_META_GAP: f64 = 2.0;
const PICKER_META_H: f64 = 16.0;
const PICKER_MIN_W: f64 = 380.0;

type Rgba = (f64, f64, f64, f64);

const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0)
}

/// `.onboarding-root` background.
const PICKER_PANEL_BG: Rgba = rgb(0x19, 0x19, 0x19);
/// `.update-card` background, and the update window's secondary button.
const PICKER_CARD_BG: Rgba = rgb(0x22, 0x22, 0x22);
const PICKER_CARD_HOVER: Rgba = rgb(0x2b, 0x2b, 0x2b);
const PICKER_CARD_PRESSED: Rgba = rgb(0x30, 0x30, 0x30);
/// `.onboarding-text-button` hover and active washes.
const PICKER_WASH: Rgba = (243.0 / 255.0, 240.0 / 255.0, 236.0 / 255.0, 0.06);
const PICKER_WASH_ACTIVE: Rgba = (243.0 / 255.0, 240.0 / 255.0, 236.0 / 255.0, 0.10);
/// Headline and value text.
const PICKER_TEXT: Rgba = rgb(0xf3, 0xf0, 0xec);
/// `.onboarding-copy` and `.update-row-key`.
const PICKER_TEXT_MUTED: Rgba = rgb(0xa3, 0xa0, 0x9c);
/// `.onboarding-text-button` idle colour.
const PICKER_CLOSE_IDLE: Rgba = rgb(0xb9, 0xb4, 0xad);
/// The primary button orange.
const PICKER_ACCENT: Rgba = rgb(0xff, 0x67, 0x0d);

fn picker_row_width(card_count: usize) -> f64 {
    card_count as f64 * PICKER_CARD_W + card_count.saturating_sub(1) as f64 * PICKER_CARD_GAP
}

fn picker_cards_top() -> f64 {
    PICKER_INSET + PICKER_HEADLINE_H + PICKER_COPY_GAP + PICKER_COPY_H + PICKER_CARDS_GAP
}

fn picker_panel_size(card_count: usize) -> (i32, i32) {
    let width = (PICKER_INSET * 2.0 + picker_row_width(card_count)).max(PICKER_MIN_W);
    let height = picker_cards_top() + PICKER_CARD_H + PICKER_INSET;
    (width as i32, height as i32)
}

/// Cards sit in one row centred in the panel, however wide the panel is.
fn picker_card_rect(index: usize, card_count: usize) -> PickerRect {
    let (panel_width, _) = picker_panel_size(card_count);
    let left = (f64::from(panel_width) - picker_row_width(card_count)) / 2.0;
    PickerRect {
        x: left + index as f64 * (PICKER_CARD_W + PICKER_CARD_GAP),
        y: picker_cards_top(),
        width: PICKER_CARD_W,
        height: PICKER_CARD_H,
    }
}

#[derive(Clone, Copy)]
struct PickerRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl PickerRect {
    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

fn picker_round_rect(context: &cairo::Context, rect: PickerRect, radius: f64) {
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(
        rect.x + rect.width - radius,
        rect.y + radius,
        radius,
        -PI / 2.0,
        0.0,
    );
    context.arc(
        rect.x + rect.width - radius,
        rect.y + rect.height - radius,
        radius,
        0.0,
        PI / 2.0,
    );
    context.arc(
        rect.x + radius,
        rect.y + rect.height - radius,
        radius,
        PI / 2.0,
        PI,
    );
    context.arc(rect.x + radius, rect.y + radius, radius, PI, PI * 1.5);
    context.close_path();
}

#[derive(Clone, Copy)]
enum PickerAlign {
    Left,
    Center,
}

/// Pango with hinting switched off, matching the rest of the overlay so
/// measured and painted text stays identical.
fn set_font_options(context: &pango::Context) {
    if let Ok(mut options) = cairo::FontOptions::new() {
        options.set_hint_style(cairo::HintStyle::None);
        options.set_hint_metrics(cairo::HintMetrics::Off);
        pangocairo::functions::context_set_font_options(context, Some(&options));
    }
}

fn picker_layout(
    context: &pango::Context,
    text: &str,
    size: f64,
    weight: pango::Weight,
) -> pango::Layout {
    let layout = pango::Layout::new(context);
    let mut description = pango::FontDescription::new();
    description.set_family(crate::typography::UI_FONT_FAMILY);
    description.set_absolute_size(size * f64::from(pango::SCALE));
    description.set_weight(weight);
    layout.set_font_description(Some(&description));
    layout.set_text(text);
    layout
}

#[allow(clippy::too_many_arguments)]
fn draw_picker_text(
    context: &cairo::Context,
    pango_context: &pango::Context,
    rect: PickerRect,
    text: &str,
    size: f64,
    weight: pango::Weight,
    color: Rgba,
    align: PickerAlign,
    tracking: f64,
) {
    let layout = picker_layout(pango_context, text, size, weight);
    if tracking != 0.0 {
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_letter_spacing(
            (tracking * f64::from(pango::SCALE)).round() as i32,
        ));
        layout.set_attributes(Some(&attrs));
    }
    let (_, logical) = layout.extents();
    let text_width = f64::from(logical.width()) / f64::from(pango::SCALE);
    let text_height = f64::from(logical.height()) / f64::from(pango::SCALE);
    // Pango clips and ellipsizes at the slot width instead of measuring
    // candidate strings by hand.
    layout.set_width((rect.width * f64::from(pango::SCALE)).round() as i32);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_single_paragraph_mode(true);
    let x = match align {
        PickerAlign::Left => rect.x,
        PickerAlign::Center => rect.x + ((rect.width - text_width) / 2.0).max(0.0),
    };
    let y = rect.y + (rect.height - text_height) / 2.0;
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.move_to(x, y);
    pangocairo::functions::show_layout(context, &layout);
}

fn fill_round_rect(context: &cairo::Context, rect: PickerRect, radius: f64, color: Rgba) {
    picker_round_rect(context, rect, radius);
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = context.fill();
}

/// The display as a plain rectangle at its real aspect ratio, centred in the
/// card's preview box, with the display number inside it.
fn draw_picker_preview(
    context: &cairo::Context,
    pango_context: &pango::Context,
    preview: PickerRect,
    card: &PickerCard,
    active: bool,
) {
    let width = f64::from(card.choice.width.max(1));
    let height = f64::from(card.choice.height.max(1));
    let scale = (preview.width / width).min(preview.height / height);
    let screen = PickerRect {
        x: preview.x + (preview.width - width * scale) / 2.0,
        y: preview.y + (preview.height - height * scale) / 2.0,
        width: width * scale,
        height: height * scale,
    };
    fill_round_rect(
        context,
        screen,
        PICKER_PREVIEW_RADIUS,
        if active {
            PICKER_WASH_ACTIVE
        } else {
            PICKER_WASH
        },
    );
    draw_picker_text(
        context,
        pango_context,
        screen,
        &card.display_number.to_string(),
        20.0,
        pango::Weight::Semibold,
        if active { PICKER_ACCENT } else { PICKER_TEXT },
        PickerAlign::Center,
        0.0,
    );
}

fn draw_picker_card(
    context: &cairo::Context,
    pango_context: &pango::Context,
    rect: PickerRect,
    card: &PickerCard,
    hovered: bool,
    pressed: bool,
) {
    let fill = if pressed {
        PICKER_CARD_PRESSED
    } else if hovered {
        PICKER_CARD_HOVER
    } else {
        PICKER_CARD_BG
    };
    fill_round_rect(context, rect, PICKER_CARD_RADIUS, fill);

    let content = PickerRect {
        x: rect.x + PICKER_CARD_PAD,
        y: rect.y + PICKER_CARD_PAD,
        width: rect.width - PICKER_CARD_PAD * 2.0,
        height: rect.height - PICKER_CARD_PAD * 2.0,
    };
    let preview = PickerRect {
        x: content.x,
        y: content.y,
        width: PICKER_PREVIEW_W,
        height: PICKER_PREVIEW_H,
    };
    draw_picker_preview(context, pango_context, preview, card, hovered || pressed);

    let name = PickerRect {
        x: content.x,
        y: preview.y + preview.height + PICKER_PREVIEW_GAP,
        width: content.width,
        height: PICKER_NAME_H,
    };
    draw_picker_text(
        context,
        pango_context,
        name,
        &format!("Display {}", card.display_number),
        14.0,
        pango::Weight::Medium,
        PICKER_TEXT,
        PickerAlign::Left,
        0.0,
    );

    let mut metadata = format!(
        "{} · {} × {}",
        card.choice.connector, card.choice.width, card.choice.height
    );
    if card.choice.is_primary {
        metadata.push_str(" · Primary");
    }
    let meta = PickerRect {
        x: content.x,
        y: name.y + name.height + PICKER_META_GAP,
        width: content.width,
        height: PICKER_META_H,
    };
    draw_picker_text(
        context,
        pango_context,
        meta,
        &metadata,
        13.0,
        pango::Weight::Normal,
        PICKER_TEXT_MUTED,
        PickerAlign::Left,
        0.0,
    );
}

fn close_button_rect(panel_width: f64) -> PickerRect {
    PickerRect {
        x: panel_width - PICKER_CLOSE_MARGIN - PICKER_CLOSE_SIZE,
        y: PICKER_CLOSE_MARGIN,
        width: PICKER_CLOSE_SIZE,
        height: PICKER_CLOSE_SIZE,
    }
}

fn draw_close_button(context: &cairo::Context, rect: PickerRect, hovered: bool) {
    if hovered {
        fill_round_rect(context, rect, rect.width / 2.0, PICKER_WASH);
    }
    let center = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let half = 4.5;
    context.set_line_width(1.5);
    context.set_line_cap(cairo::LineCap::Round);
    context.move_to(center.0 - half, center.1 - half);
    context.line_to(center.0 + half, center.1 + half);
    context.move_to(center.0 + half, center.1 - half);
    context.line_to(center.0 - half, center.1 + half);
    let color = if hovered {
        PICKER_TEXT
    } else {
        PICKER_CLOSE_IDLE
    };
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = context.stroke();
}

fn draw_picker_panel(
    context: &cairo::Context,
    width: i32,
    height: i32,
    cards: &[PickerCard],
    hovered: i32,
    pressed: i32,
    close_hovered: bool,
) {
    context.set_antialias(cairo::Antialias::Best);
    let panel = PickerRect {
        x: 0.0,
        y: 0.0,
        width: width as f64,
        height: height as f64,
    };
    // Opaque and borderless, like the onboarding sheet; only the area
    // outside the rounded corners stays transparent.
    fill_round_rect(context, panel, PICKER_PANEL_RADIUS, PICKER_PANEL_BG);

    let pango_context = pangocairo::functions::create_context(context);
    set_font_options(&pango_context);

    let text_slot = |y: f64, height: f64| PickerRect {
        x: PICKER_INSET,
        y,
        width: panel.width - PICKER_INSET * 2.0,
        height,
    };
    draw_picker_text(
        context,
        &pango_context,
        text_slot(PICKER_INSET, PICKER_HEADLINE_H),
        "Choose a display",
        28.0,
        pango::Weight::Semibold,
        PICKER_TEXT,
        PickerAlign::Center,
        -0.6,
    );
    draw_picker_text(
        context,
        &pango_context,
        text_slot(
            PICKER_INSET + PICKER_HEADLINE_H + PICKER_COPY_GAP,
            PICKER_COPY_H,
        ),
        "Pick the screen to capture.",
        15.0,
        pango::Weight::Normal,
        PICKER_TEXT_MUTED,
        PickerAlign::Center,
        0.0,
    );
    draw_close_button(context, close_button_rect(panel.width), close_hovered);

    for (index, card) in cards.iter().enumerate() {
        draw_picker_card(
            context,
            &pango_context,
            picker_card_rect(index, cards.len()),
            card,
            hovered == index as i32,
            pressed == index as i32,
        );
    }
}

fn select_monitor_interactive(
    display: &gdk::Display,
    monitors: &[gdk::Monitor],
) -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    install_picker_canvas_css(display);
    let sorted = sorted_monitor_indices(monitors);
    let cards: Vec<_> = sorted
        .iter()
        .enumerate()
        .map(|(display_number, &monitor_index)| PickerCard {
            monitor_index: monitor_index as i32,
            display_number: display_number + 1,
            choice: MonitorChoice::from_monitor(
                monitor_index as u32,
                &monitors[monitor_index],
                is_primary_guess(monitors, monitor_index),
            ),
        })
        .collect();
    let (panel_width, panel_height) = picker_panel_size(cards.len());
    let result = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);
    let panel = Window::builder()
        .title("ApexShot Display Picker")
        .decorated(false)
        .resizable(false)
        .modal(true)
        .css_classes(["apexshot-monitor-picker-canvas"])
        .build();
    panel.set_default_size(panel_width, panel_height);
    // A plain toplevel lands wherever the compositor puts new windows. On a
    // layer-shell compositor, an overlay surface with no anchors is centred
    // on its output instead, so the picker opens in the middle of the
    // primary display.
    if gtk4_layer_shell::is_supported() {
        use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
        panel.init_layer_shell();
        panel.set_layer(Layer::Overlay);
        panel.set_namespace(Some("apexshot-monitor-picker"));
        panel.set_keyboard_mode(KeyboardMode::Exclusive);
        let primary = (0..monitors.len())
            .find(|&index| is_primary_guess(monitors, index))
            .unwrap_or(0);
        panel.set_monitor(Some(&monitors[primary]));
    }

    let hovered = Rc::new(Cell::new(0));
    let pressed = Rc::new(Cell::new(-1));
    let close_hovered = Rc::new(Cell::new(false));
    let drawing = DrawingArea::new();
    drawing.set_content_width(panel_width);
    drawing.set_content_height(panel_height);
    drawing.set_size_request(panel_width, panel_height);
    drawing.set_focusable(true);
    {
        let cards = cards.clone();
        let hovered = hovered.clone();
        let pressed = pressed.clone();
        let close_hovered = close_hovered.clone();
        drawing.set_draw_func(move |_, context, width, height| {
            draw_picker_panel(
                context,
                width,
                height,
                &cards,
                hovered.get(),
                pressed.get(),
                close_hovered.get(),
            );
        });
    }

    let card_hit = {
        let card_count = cards.len();
        move |x: f64, y: f64| {
            (0..card_count)
                .find(|&index| picker_card_rect(index, card_count).contains(x, y))
                .map(|index| index as i32)
        }
    };
    let motion = EventControllerMotion::new();
    {
        let hovered = hovered.clone();
        let close_hovered = close_hovered.clone();
        let drawing = drawing.clone();
        motion.connect_motion(move |_, x, y| {
            let next = card_hit(x, y).unwrap_or(-1);
            let next_close = close_button_rect(panel_width as f64).contains(x, y);
            if hovered.replace(next) != next || close_hovered.replace(next_close) != next_close {
                drawing.queue_draw();
            }
        });
    }
    {
        let hovered = hovered.clone();
        let close_hovered = close_hovered.clone();
        let drawing = drawing.clone();
        motion.connect_leave(move |_| {
            if hovered.replace(-1) != -1 || close_hovered.replace(false) {
                drawing.queue_draw();
            }
        });
    }
    drawing.add_controller(motion);
    let click = GestureClick::builder().button(1).build();
    {
        let pressed = pressed.clone();
        let drawing = drawing.clone();
        click.connect_pressed(move |_, _, x, y| {
            pressed.set(card_hit(x, y).unwrap_or(-1));
            drawing.queue_draw();
        });
    }
    {
        let cards = cards.clone();
        let result = result.clone();
        let main_loop = main_loop.clone();
        let panel = panel.clone();
        let pressed = pressed.clone();
        let drawing = drawing.clone();
        click.connect_released(move |_, _, x, y| {
            let hit = card_hit(x, y);
            let pressed_index = pressed.replace(-1);
            if hit == Some(pressed_index) && pressed_index >= 0 {
                *result.borrow_mut() = Some(cards[pressed_index as usize].monitor_index);
                panel.close();
                main_loop.quit();
                return;
            }
            if close_button_rect(panel_width as f64).contains(x, y) {
                *result.borrow_mut() = Some(-1);
                panel.close();
                main_loop.quit();
                return;
            }
            drawing.queue_draw();
        });
    }
    drawing.add_controller(click);
    panel.set_child(Some(&drawing));

    let key = EventControllerKey::new();
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        let panel = panel.clone();
        let cards = cards.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            use gtk4::gdk::Key;
            let chosen = match keyval {
                Key::_1 => cards.first().map(|card| card.monitor_index),
                Key::_2 => cards.get(1).map(|card| card.monitor_index),
                Key::_3 => cards.get(2).map(|card| card.monitor_index),
                Key::_4 => cards.get(3).map(|card| card.monitor_index),
                Key::_5 => cards.get(4).map(|card| card.monitor_index),
                Key::_6 => cards.get(5).map(|card| card.monitor_index),
                Key::_7 => cards.get(6).map(|card| card.monitor_index),
                Key::_8 => cards.get(7).map(|card| card.monitor_index),
                Key::_9 => cards.get(8).map(|card| card.monitor_index),
                Key::Escape => Some(-1),
                Key::Return | Key::KP_Enter | Key::space => {
                    cards.first().map(|card| card.monitor_index)
                }
                _ => None,
            };
            if let Some(chosen) = chosen {
                *result.borrow_mut() = Some(chosen);
                panel.close();
                main_loop.quit();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    panel.add_controller(key);
    {
        let result = result.clone();
        let main_loop = main_loop.clone();
        panel.connect_close_request(move |_| {
            if result.borrow().is_none() {
                *result.borrow_mut() = Some(-1);
            }
            main_loop.quit();
            glib::Propagation::Proceed
        });
    }
    panel.present();
    let _ = drawing.grab_focus();
    main_loop.run();
    panel.hide();
    panel.destroy();
    while glib::MainContext::default().iteration(false) {}
    std::thread::sleep(std::time::Duration::from_millis(120));
    while glib::MainContext::default().iteration(false) {}
    let code = result.borrow().unwrap_or(-1);
    if code < 0 || code as usize >= monitors.len() {
        eprintln!("[monitor-picker] cancelled");
        return Err(SelectionError::Cancelled);
    }
    let monitor = monitors[code as usize].clone();
    let choice = MonitorChoice::from_monitor(
        code as u32,
        &monitor,
        is_primary_guess(monitors, code as usize),
    );
    eprintln!(
        "[monitor-picker] selected index={} name={} geom={}x{}+{}+{}",
        choice.index, choice.connector, choice.width, choice.height, choice.x, choice.y
    );
    Ok((monitor, choice))
}

fn install_picker_canvas_css(display: &gdk::Display) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let provider = CssProvider::new();
        provider.load_from_data(
            "window.apexshot-monitor-picker-canvas, window.apexshot-monitor-picker-canvas > * { background: transparent; }",
        );
        gtk4::style_context_add_provider_for_display(
            display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}

/// Resolve a previously chosen monitor by matching geometry origin.
///
/// Used when the picker ran before the overlay Application started and we need
/// to re-bind the same logical output inside `setup_window`.
pub fn find_monitor_at(display: &gdk::Display, x: i32, y: i32) -> Option<gdk::Monitor> {
    let monitors = list_monitors(display);
    monitors
        .into_iter()
        .find(|m| {
            let g = m.geometry();
            g.x() == x && g.y() == y
        })
        .or_else(|| {
            // Nearest origin match (handles minor geometry drift).
            list_monitors(display).into_iter().min_by_key(|m| {
                let g = m.geometry();
                (g.x() - x).unsigned_abs() + (g.y() - y).unsigned_abs()
            })
        })
}

/// Public helper for callers that only need geometry (no GTK Window yet).
pub fn select_target_monitor_choice() -> Result<MonitorChoice, SelectionError> {
    select_target_monitor().map(|(_, choice)| choice)
}

/// Choose a primary monitor as an anchor without showing the picker.
pub(crate) fn default_target_monitor_choice() -> Result<MonitorChoice, SelectionError> {
    let display = gdk::Display::default()
        .ok_or_else(|| SelectionError::InitError("No display found for monitor picker".into()))?;
    let monitors = list_monitors(&display);
    if monitors.is_empty() {
        return Err(SelectionError::InitError("No monitor found".into()));
    }
    let index = (0..monitors.len())
        .find(|&index| is_primary_guess(&monitors, index))
        .unwrap_or(0);
    Ok(MonitorChoice::from_monitor(
        index as u32,
        &monitors[index],
        true,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_choice_from_fields() {
        // Pure unit: struct construction without GDK.
        let c = MonitorChoice {
            index: 1,
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            connector: "DP-2".into(),
            is_primary: false,
        };
        assert_eq!(c.connector, "DP-2");
        assert_eq!(c.width, 2560);
    }

    #[test]
    fn picker_geometry_centres_the_cards() {
        // One card keeps the 380px minimum and sits in the middle of it.
        assert_eq!(picker_panel_size(1), (380, 338));
        let only = picker_card_rect(0, 1);
        assert_eq!(only.x, 80.0);
        assert_eq!(only.y, 120.0);
        assert_eq!((only.width, only.height), (220.0, 182.0));
        assert_eq!(only.y + only.height + PICKER_INSET, 338.0);

        // Three cards: 36 inset either side, 12 between cards.
        assert_eq!(picker_panel_size(3), (756, 338));
        assert_eq!(picker_card_rect(0, 3).x, 36.0);
        assert_eq!(picker_card_rect(1, 3).x, 268.0);
        assert_eq!(picker_card_rect(2, 3).x + 220.0 + 36.0, 756.0);
    }
}
