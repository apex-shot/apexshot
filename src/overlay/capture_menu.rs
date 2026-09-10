//! Compact GTK capture toolbar rendered from the C++ toolbar's own geometry.
//!
//! Drawing it with Cairo keeps GTK themes from substituting legacy widgets or
//! text glyphs for the C++ visual contract.

use super::monitor_picker::{find_monitor_at, MonitorChoice};
use gtk4::{
    cairo, gdk, glib, prelude::*, CssProvider, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureClick, Window,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMenuAction {
    Cancel,
    Display,
    Window,
    Area,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

const PANEL_WIDTH: i32 = 579;
const PANEL_HEIGHT: i32 = 82;
const ITEM_WIDTHS: [f64; 8] = [66.0, 58.0, 76.0, 76.0, 66.0, 62.0, 68.0, 52.0];
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

fn item_rect(index: usize) -> Rect {
    let mut x = 8.0;
    for prior in 0..index {
        x += ITEM_WIDTHS[prior];
        if separator_after(prior) {
            x += 13.0;
        }
    }
    Rect {
        x,
        y: 8.0,
        width: ITEM_WIDTHS[index],
        height: 66.0,
    }
}

fn item_at(x: f64, y: f64) -> Option<usize> {
    (0..8).find(|&i| item_rect(i).contains(x, y))
}

fn item_is_disabled(index: usize, state: MenuState) -> bool {
    (state.recording && matches!(index, 3 | 4))
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

fn draw_text(
    context: &cairo::Context,
    rect: Rect,
    text: &str,
    size: f64,
    weight: cairo::FontWeight,
    color: (f64, f64, f64, f64),
) {
    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        cairo::FontSlant::Normal,
        weight,
    );
    context.set_font_size(size);
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    if let Ok(metrics) = context.text_extents(text) {
        context.move_to(
            rect.x + (rect.width - metrics.width()) / 2.0 - metrics.x_bearing(),
            rect.y + (rect.height - metrics.height()) / 2.0 - metrics.y_bearing(),
        );
        let _ = context.show_text(text);
    }
}

fn draw_icon(
    context: &cairo::Context,
    index: usize,
    x: f64,
    y: f64,
    color: (f64, f64, f64, f64),
    state: MenuState,
) {
    let _ = context.save();
    // Cairo keeps its current path until it is explicitly consumed.  Each
    // C++ QPainter icon is an isolated path, so start/end the Rust port the
    // same way; otherwise a prior glyph can be stroked with the timer hands.
    context.new_path();
    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.set_line_width(if index == 7 { 2.6 } else { 2.0 });
    context.set_line_cap(cairo::LineCap::Round);
    context.set_line_join(cairo::LineJoin::Round);
    match index {
        0 => {
            rounded_rect(
                context,
                Rect {
                    x: x - 10.0,
                    y: y - 7.0,
                    width: 20.0,
                    height: 15.0,
                },
                2.5,
            );
            let _ = context.stroke();
            context.arc(x + 4.5, y - 2.5, 1.5, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.move_to(x - 7.0, y + 5.0);
            context.line_to(x - 2.0, y);
            context.line_to(x + 2.0, y + 4.0);
            let _ = context.stroke();
        }
        1 => {
            rounded_rect(
                context,
                Rect {
                    x: x - 10.0,
                    y: y - 7.0,
                    width: 14.0,
                    height: 14.0,
                },
                2.5,
            );
            let _ = context.stroke();
            context.move_to(x + 5.0, y - 4.0);
            context.line_to(x + 10.0, y - 7.0);
            context.line_to(x + 10.0, y + 7.0);
            context.line_to(x + 5.0, y + 4.0);
            let _ = context.stroke();
        }
        2 => {
            rounded_rect(
                context,
                Rect {
                    x: x - 10.0,
                    y: y - 8.0,
                    width: 20.0,
                    height: 14.0,
                },
                2.0,
            );
            let _ = context.stroke();
            context.move_to(x, y + 6.0);
            context.line_to(x, y + 10.0);
            context.move_to(x - 5.0, y + 10.0);
            context.line_to(x + 5.0, y + 10.0);
            let _ = context.stroke();
        }
        3 => {
            rounded_rect(
                context,
                Rect {
                    x: x - 10.0,
                    y: y - 8.0,
                    width: 20.0,
                    height: 16.0,
                },
                3.0,
            );
            let _ = context.stroke();
            context.move_to(x - 10.0, y - 3.0);
            context.line_to(x + 10.0, y - 3.0);
            let _ = context.stroke();
            context.arc(x - 6.0, y - 5.5, 0.75, 0.0, PI * 2.0);
            let _ = context.fill();
        }
        4 => {
            for (x1, y1, x2, y2) in [
                (x - 9.0, y - 3.0, x - 9.0, y - 9.0),
                (x - 9.0, y - 9.0, x - 3.0, y - 9.0),
                (x + 3.0, y - 9.0, x + 9.0, y - 9.0),
                (x + 9.0, y - 9.0, x + 9.0, y - 3.0),
                (x - 9.0, y + 3.0, x - 9.0, y + 9.0),
                (x - 9.0, y + 9.0, x - 3.0, y + 9.0),
                (x + 3.0, y + 9.0, x + 9.0, y + 9.0),
                (x + 9.0, y + 9.0, x + 9.0, y + 3.0),
            ] {
                context.move_to(x1, y1);
                context.line_to(x2, y2);
            }
            let _ = context.stroke();
        }
        5 if state.recording => {
            rounded_rect(
                context,
                Rect {
                    x: x - 4.5,
                    y: y - 9.0,
                    width: 9.0,
                    height: 14.0,
                },
                4.5,
            );
            let _ = context.stroke();
            context.move_to(x - 8.0, y + 1.0);
            context.line_to(x - 8.0, y + 4.0);
            context.arc(x, y + 4.5, 8.0, 0.0, PI);
            context.move_to(x, y + 10.0);
            context.line_to(x, y + 13.0);
            context.move_to(x - 5.0, y + 13.0);
            context.line_to(x + 5.0, y + 13.0);
            if !state.microphone {
                context.move_to(x - 10.0, y - 10.0);
                context.line_to(x + 10.0, y + 11.0);
            }
            let _ = context.stroke();
        }
        5 => draw_text(
            context,
            Rect {
                x: x - 12.0,
                y: y - 10.0,
                width: 24.0,
                height: 20.0,
            },
            "Aa",
            13.0,
            cairo::FontWeight::Bold,
            color,
        ),
        6 if state.recording => {
            context.move_to(x - 10.0, y - 3.0);
            context.line_to(x - 5.0, y - 3.0);
            context.line_to(x + 1.0, y - 9.0);
            context.line_to(x + 1.0, y + 9.0);
            context.line_to(x - 5.0, y + 3.0);
            context.line_to(x - 10.0, y + 3.0);
            context.close_path();
            let _ = context.stroke();
            if state.speaker {
                context.arc(
                    x + 5.0,
                    y,
                    8.0,
                    -55.0_f64.to_radians(),
                    55.0_f64.to_radians(),
                );
            } else {
                context.move_to(x - 10.0, y - 10.0);
                context.line_to(x + 10.0, y + 10.0);
            }
            let _ = context.stroke();
        }
        6 => {
            context.arc(x, y + 1.0, 8.0, 0.0, PI * 2.0);
            let _ = context.stroke();
            context.new_path();
            context.move_to(x, y - 7.0);
            context.line_to(x, y - 10.0);
            context.move_to(x - 3.0, y - 10.0);
            context.line_to(x + 3.0, y - 10.0);
            context.move_to(x, y + 1.0);
            context.line_to(x, y - 4.0);
            context.move_to(x, y + 1.0);
            context.line_to(x + 4.0, y + 3.0);
            let _ = context.stroke();
        }
        7 => {
            context.move_to(x - 6.0, y - 6.0);
            context.line_to(x + 6.0, y + 6.0);
            context.move_to(x + 6.0, y - 6.0);
            context.line_to(x - 6.0, y + 6.0);
            let _ = context.stroke();
        }
        _ => {}
    }
    context.new_path();
    let _ = context.restore();
}

fn label_for(index: usize, state: MenuState) -> String {
    match index {
        5 if state.recording => "Mic".into(),
        6 if state.recording => "Speaker".into(),
        6 if state.timer_seconds > 0 => format!("{}s", state.timer_seconds),
        _ => LABELS[index].into(),
    }
}

fn draw_menu(context: &cairo::Context, state: MenuState, hovered: i32) {
    context.set_antialias(cairo::Antialias::Best);
    let panel = Rect {
        x: 1.5,
        y: 1.5,
        width: 576.0,
        height: 76.0,
    };
    rounded_rect(
        context,
        Rect {
            y: panel.y + 3.0,
            ..panel
        },
        22.0,
    );
    context.set_source_rgba(0.0, 0.0, 0.0, 105.0 / 255.0);
    let _ = context.fill();
    rounded_rect(context, panel, 22.0);
    context.set_source_rgba(25.0 / 255.0, 25.0 / 255.0, 28.0 / 255.0, 246.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 54.0 / 255.0);
    context.set_line_width(1.2);
    let _ = context.stroke();
    let first = item_rect(0);
    let second = item_rect(1);
    let mode_group = Rect {
        x: first.x,
        y: first.y,
        width: second.x + second.width - first.x,
        height: first.height,
    };
    rounded_rect(context, mode_group, 15.0);
    context.set_source_rgba(5.0 / 255.0, 5.0 / 255.0, 7.0 / 255.0, 235.0 / 255.0);
    let _ = context.fill_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 18.0 / 255.0);
    context.set_line_width(1.0);
    let _ = context.stroke();
    for index in 0..8 {
        let cell = item_rect(index);
        let disabled = item_is_disabled(index, state);
        let active = item_is_active(index, state);
        let over = hovered == index as i32 && !disabled;
        if active || over {
            let highlight = Rect {
                x: cell.x + 4.0,
                y: cell.y + 4.0,
                width: cell.width - 8.0,
                height: cell.height - 8.0,
            };
            rounded_rect(context, highlight, 13.0);
            if active {
                context.set_source_rgba(1.0, 102.0 / 255.0, 0.0, 205.0 / 255.0);
            } else {
                context.set_source_rgba(1.0, 1.0, 1.0, 24.0 / 255.0);
            }
            let _ = context.fill_preserve();
            if over && !active {
                context.set_source_rgba(1.0, 102.0 / 255.0, 0.0, 170.0 / 255.0);
                context.set_line_width(1.0);
                let _ = context.stroke();
            } else {
                context.new_path();
            }
        }
        let color = if active {
            (1.0, 1.0, 1.0, 1.0)
        } else if disabled {
            (115.0 / 255.0, 115.0 / 255.0, 122.0 / 255.0, 1.0)
        } else {
            (245.0 / 255.0, 245.0 / 255.0, 247.0 / 255.0, 1.0)
        };
        let (cx, cy) = cell.center();
        draw_icon(
            context,
            index,
            cx,
            if index == 7 { cy } else { cell.y + 25.0 },
            color,
            state,
        );
        if index != 7 {
            let label_color = if active {
                (1.0, 1.0, 1.0, 1.0)
            } else if disabled {
                (115.0 / 255.0, 115.0 / 255.0, 122.0 / 255.0, 1.0)
            } else {
                (164.0 / 255.0, 164.0 / 255.0, 172.0 / 255.0, 1.0)
            };
            draw_text(
                context,
                Rect {
                    x: cell.x,
                    y: cell.y + 40.0,
                    width: cell.width,
                    height: 18.0,
                },
                &label_for(index, state),
                11.0,
                if active {
                    cairo::FontWeight::Bold
                } else {
                    cairo::FontWeight::Normal
                },
                label_color,
            );
        }
        if separator_after(index) {
            context.set_source_rgba(1.0, 1.0, 1.0, 35.0 / 255.0);
            context.set_line_width(1.0);
            context.move_to(cell.x + cell.width + 6.5, 20.0);
            context.line_to(cell.x + cell.width + 6.5, PANEL_HEIGHT as f64 - 20.0);
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
    let Some(display) = gdk::Display::default() else {
        return CaptureMenuResult::default();
    };
    install_transparent_window_css(&display);
    let result = Rc::new(RefCell::new(None));
    let main_loop = glib::MainLoop::new(None, false);
    let window = Window::builder()
        .title("ApexShot Capture")
        .decorated(false)
        .resizable(false)
        .css_classes(["apexshot-capture-menu"])
        .build();
    window.set_default_size(PANEL_WIDTH, PANEL_HEIGHT);
    if gtk4_layer_shell::is_supported() {
        if let Some(monitor) = find_monitor_at(&display, target.x, target.y) {
            window.init_layer_shell();
            window.set_layer(Layer::Overlay);
            window.set_anchor(Edge::Top, true);
            window.set_margin(Edge::Top, 28);
            window.set_monitor(Some(&monitor));
            window.set_keyboard_mode(KeyboardMode::Exclusive);
            window.set_namespace(Some("apexshot-capture-menu"));
        }
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
    drawing.set_content_width(PANEL_WIDTH);
    drawing.set_content_height(PANEL_HEIGHT);
    drawing.set_size_request(PANEL_WIDTH, PANEL_HEIGHT);
    drawing.set_focusable(true);
    {
        let state = state.clone();
        let hovered = hovered.clone();
        drawing.set_draw_func(move |_, context, _, _| {
            draw_menu(context, *state.borrow(), hovered.get())
        });
    }
    let motion = EventControllerMotion::new();
    {
        let state = state.clone();
        let hovered = hovered.clone();
        let drawing = drawing.clone();
        motion.connect_motion(move |_, x, y| {
            let next = item_at(x, y)
                .filter(|&i| !item_is_disabled(i, *state.borrow()))
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
        click.connect_pressed(move |_, _, x, y| {
            let Some(index) = item_at(x, y) else { return };
            let mut menu = state.borrow_mut();
            if item_is_disabled(index, *menu) {
                return;
            }
            match index {
                0 => menu.recording = false,
                1 => {
                    menu.recording = true;
                    menu.ocr = false;
                    menu.timer_seconds = 0;
                }
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
    window.present();
    let _ = drawing.grab_focus();
    main_loop.run();
    window.hide();
    window.destroy();
    while glib::MainContext::default().iteration(false) {}
    let selected = result.borrow().unwrap_or_default();
    selected
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
        assert!(item_is_disabled(3, video));
        assert!(item_is_disabled(4, video));
    }
    #[test]
    fn item_rects_match_the_cpp_toolbar_layout() {
        assert_eq!(item_rect(0).x, 8.0);
        assert_eq!(item_rect(1).x, 74.0);
        assert_eq!(item_rect(2).x, 145.0);
        assert_eq!(item_rect(5).x, 376.0);
        assert_eq!(item_rect(7).x, 519.0);
    }
}
