//! Multi-monitor display picker for the Rust GTK overlay.
//!
//! Mirrors the C++ `MonitorPicker` flow:
//! - 1 monitor → that monitor immediately (no UI)
//! - multi → floating "Select a display" panel
//! - Esc / Cancel → cancelled
//! - number keys 1–9 select by sorted left-to-right order
//!
//! The picker is metadata-only: freeze/capture happens after dismiss so the
//! panel never appears in the frozen background.

use super::api::SelectionError;
use gtk4::{
    gdk, glib, prelude::*, Align, Box as GtkBox, Button, CssProvider, EventControllerKey, Label,
    Orientation, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

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
            .unwrap_or_else(|| format!("Display {}", index + 1));
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
/// Cancelled when the user presses Esc / clicks Cancel / closes the panel.
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

fn select_monitor_interactive(
    display: &gdk::Display,
    monitors: &[gdk::Monitor],
) -> Result<(gdk::Monitor, MonitorChoice), SelectionError> {
    install_picker_css(display);

    let result: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);

    // Transient floating panel — does not cover the desktop (live desktop stays).
    // Compositors place the modal on the active/focused display.
    let panel = Window::builder()
        .title("Select a display")
        .decorated(false)
        .resizable(false)
        .modal(true)
        .css_classes(["apexshot-monitor-picker"])
        .build();
    panel.set_default_size(-1, -1);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_margin_top(28);
    root.set_margin_bottom(20);
    root.set_margin_start(36);
    root.set_margin_end(36);
    root.add_css_class("apexshot-monitor-picker-root");

    let title = Label::new(Some("Select a display"));
    title.add_css_class("apexshot-monitor-picker-title");
    title.set_halign(Align::Center);
    root.append(&title);

    let n_keys = monitors.len().min(9);
    let hint = Label::new(Some(&format!(
        "Click a display  ·  Esc to cancel  ·  1–{n_keys}"
    )));
    hint.add_css_class("apexshot-monitor-picker-hint");
    hint.set_halign(Align::Center);
    hint.set_margin_top(4);
    hint.set_margin_bottom(22);
    root.append(&hint);

    let row = GtkBox::new(Orientation::Horizontal, 16);
    row.set_halign(Align::Center);

    let sorted = sorted_monitor_indices(monitors);
    let mut first_card: Option<Button> = None;

    for (display_ord, &mon_index) in sorted.iter().enumerate() {
        let monitor = &monitors[mon_index];
        let primary = is_primary_guess(monitors, mon_index);
        let choice = MonitorChoice::from_monitor(mon_index as u32, monitor, primary);
        let card = build_monitor_card(display_ord, &choice);

        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        let mon_index_i = mon_index as i32;
        card.connect_clicked(move |_| {
            *result_c.borrow_mut() = Some(mon_index_i);
            panel_c.close();
            loop_c.quit();
        });

        row.append(&card);
        if first_card.is_none() {
            first_card = Some(card);
        }
    }
    root.append(&row);

    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("apexshot-monitor-picker-cancel");
    cancel.set_halign(Align::Center);
    cancel.set_margin_top(12);
    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        cancel.connect_clicked(move |_| {
            *result_c.borrow_mut() = Some(-1);
            panel_c.close();
            loop_c.quit();
        });
    }
    root.append(&cancel);

    panel.set_child(Some(&root));

    // Keyboard: Esc cancel, 1–9 select by sorted order.
    let key = EventControllerKey::new();
    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        let panel_c = panel.clone();
        let sorted_keys = sorted.clone();
        key.connect_key_pressed(move |_, keyval, _, _| {
            use gtk4::gdk::Key;
            if keyval == Key::Escape {
                *result_c.borrow_mut() = Some(-1);
                panel_c.close();
                loop_c.quit();
                return glib::Propagation::Stop;
            }
            // GDK keyvals for '1'..'9'
            let digit = match keyval {
                Key::_1 => Some(0usize),
                Key::_2 => Some(1),
                Key::_3 => Some(2),
                Key::_4 => Some(3),
                Key::_5 => Some(4),
                Key::_6 => Some(5),
                Key::_7 => Some(6),
                Key::_8 => Some(7),
                Key::_9 => Some(8),
                _ => None,
            };
            if let Some(ord) = digit {
                if let Some(&mon_index) = sorted_keys.get(ord) {
                    *result_c.borrow_mut() = Some(mon_index as i32);
                    panel_c.close();
                    loop_c.quit();
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }
    panel.add_controller(key);

    {
        let result_c = result.clone();
        let loop_c = main_loop.clone();
        panel.connect_close_request(move |_| {
            if result_c.borrow().is_none() {
                *result_c.borrow_mut() = Some(-1);
            }
            loop_c.quit();
            glib::Propagation::Proceed
        });
    }

    // Size hint so the panel lays out cards before present().
    let panel_w = estimate_panel_width(monitors.len());
    panel.set_default_size(panel_w, -1);

    panel.present();
    if let Some(card) = first_card {
        card.grab_focus();
    }

    main_loop.run();

    // Force-destroy and drain so the surface is unmapped before freeze/capture.
    panel.hide();
    panel.destroy();
    // Process pending unmap events.
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

fn estimate_panel_width(n_monitors: usize) -> i32 {
    // Card ~288px + gaps + margins (mirrors C++ kPreviewW + pad).
    let card = 288;
    let gap = 16;
    let margins = 72;
    (n_monitors as i32 * card + (n_monitors as i32 - 1).max(0) * gap + margins).max(360)
}

fn build_monitor_card(display_ord: usize, choice: &MonitorChoice) -> Button {
    let card = Button::new();
    card.add_css_class("apexshot-monitor-card");
    card.set_focusable(true);
    card.set_can_focus(true);

    let col = GtkBox::new(Orientation::Vertical, 8);
    col.set_margin_top(14);
    col.set_margin_bottom(14);
    col.set_margin_start(14);
    col.set_margin_end(14);
    col.set_size_request(260, -1);

    // Preview glyph area (stylized monitor shape via CSS + labels).
    let glyph = GtkBox::new(Orientation::Vertical, 0);
    glyph.add_css_class("apexshot-monitor-glyph");
    glyph.set_size_request(260, 140);
    glyph.set_halign(Align::Fill);

    let badge_row = GtkBox::new(Orientation::Horizontal, 0);
    badge_row.set_margin_top(8);
    badge_row.set_margin_start(8);
    badge_row.set_margin_end(8);
    badge_row.set_hexpand(true);

    let num = Label::new(Some(&(display_ord + 1).to_string()));
    num.add_css_class("apexshot-monitor-badge");
    num.set_halign(Align::Start);
    badge_row.append(&num);

    if choice.is_primary {
        let spacer = Label::new(None);
        spacer.set_hexpand(true);
        badge_row.append(&spacer);
        let primary = Label::new(Some("Primary"));
        primary.add_css_class("apexshot-monitor-primary-chip");
        primary.set_halign(Align::End);
        badge_row.append(&primary);
    }

    glyph.append(&badge_row);

    let screen_fake = Label::new(None);
    screen_fake.set_vexpand(true);
    glyph.append(&screen_fake);

    col.append(&glyph);

    let title = Label::new(Some(&format!("Display {}", display_ord + 1)));
    title.add_css_class("apexshot-monitor-card-title");
    title.set_halign(Align::Start);
    title.set_xalign(0.0);
    col.append(&title);

    let sub = Label::new(Some(&format!(
        "{} × {}  ·  {}",
        choice.width, choice.height, choice.connector
    )));
    sub.add_css_class("apexshot-monitor-card-sub");
    sub.set_halign(Align::Start);
    sub.set_xalign(0.0);
    sub.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    col.append(&sub);

    card.set_child(Some(&col));
    card
}

fn install_picker_css(display: &gdk::Display) {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let provider = CssProvider::new();
        provider.load_from_data(
            r#"
            window.apexshot-monitor-picker {
                background-color: #141414;
                border-radius: 18px;
                border: 1px solid rgba(255, 255, 255, 0.09);
            }
            .apexshot-monitor-picker-root {
                background-color: #141414;
            }
            .apexshot-monitor-picker-title {
                color: rgba(255, 255, 255, 0.95);
                font-size: 18px;
                font-weight: 600;
            }
            .apexshot-monitor-picker-hint {
                color: rgba(255, 255, 255, 0.5);
                font-size: 12px;
            }
            button.apexshot-monitor-card {
                background-color: #1e1f22;
                border: 1px solid rgba(255, 255, 255, 0.11);
                border-radius: 14px;
                padding: 0;
                box-shadow: 0 3px 0 rgba(0, 0, 0, 0.25);
            }
            button.apexshot-monitor-card:hover,
            button.apexshot-monitor-card:focus {
                background-color: #24252a;
                border-color: rgba(255, 102, 0, 0.78);
                border-width: 1.5px;
            }
            .apexshot-monitor-glyph {
                background-color: #121216;
                border-radius: 10px;
                border: 1px solid rgba(255, 255, 255, 0.12);
            }
            .apexshot-monitor-badge {
                background-color: rgba(0, 0, 0, 0.7);
                color: white;
                font-size: 11px;
                font-weight: 600;
                padding: 2px 8px;
                border-radius: 6px;
                min-width: 22px;
            }
            button.apexshot-monitor-card:hover .apexshot-monitor-badge,
            button.apexshot-monitor-card:focus .apexshot-monitor-badge {
                background-color: #ff6600;
            }
            .apexshot-monitor-primary-chip {
                background-color: rgba(255, 255, 255, 0.09);
                color: rgba(255, 255, 255, 0.78);
                font-size: 10px;
                font-weight: 500;
                padding: 2px 8px;
                border-radius: 5px;
            }
            .apexshot-monitor-card-title {
                color: rgba(255, 255, 255, 0.94);
                font-size: 13px;
                font-weight: 600;
            }
            .apexshot-monitor-card-sub {
                color: rgba(255, 255, 255, 0.55);
                font-size: 11px;
            }
            button.apexshot-monitor-picker-cancel {
                background: transparent;
                border: none;
                color: rgba(255, 255, 255, 0.55);
                font-size: 12px;
                padding: 8px 16px;
                box-shadow: none;
            }
            button.apexshot-monitor-picker-cancel:hover {
                color: rgba(255, 255, 255, 0.9);
                background: transparent;
            }
            "#,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_panel_width_grows_with_monitors() {
        assert!(estimate_panel_width(2) > estimate_panel_width(1));
        assert!(estimate_panel_width(3) > estimate_panel_width(2));
    }

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
}
