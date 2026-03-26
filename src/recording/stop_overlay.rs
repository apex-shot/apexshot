use gdk4x11::X11Surface;
use gtk4::cairo;
use gtk4::gdk::{self, Key};
use gtk4::{
    glib::{self, clone},
    prelude::*,
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea,
    EventControllerKey, GestureDrag, Label, Orientation, Separator,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;
use x11rb::wrapper::ConnectionExt;
use x11rb::{
    connection::Connection,
    protocol::xproto::{self, ConnectionExt as _},
};

const BAR_W: i32 = 420;
const BAR_H: i32 = 60;
const MARGIN: i32 = 24;
const DOCK_SAFE: i32 = 64;

#[derive(Debug, Error)]
pub enum StopOverlayError {
    #[error("GTK initialization failed: {0}")]
    InitError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAction {
    Save,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingControlsParams {
    pub capture_x: i32,
    pub capture_y: i32,
    pub capture_w: i32,
    pub capture_h: i32,
    pub is_fullscreen: bool,
    pub show_timer: bool,
    pub use_shell_mask: bool,
}

pub fn run_recording_controls(
    params: RecordingControlsParams,
    stop_tx: oneshot::Sender<StopAction>,
) -> Result<(), StopOverlayError> {
    let stop_tx: Arc<Mutex<Option<oneshot::Sender<StopAction>>>> =
        Arc::new(Mutex::new(Some(stop_tx)));

    let app = Application::builder()
        .application_id("com.apexshot.recording")
        .build();

    let stop_tx_activate = stop_tx.clone();
    app.connect_activate(move |application| {
        let dim_windows = setup_dim_windows(application, params);
        setup_window(application, params, stop_tx_activate.clone(), dim_windows);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

pub fn run_recording_countdown_bar(
    params: RecordingControlsParams,
    seconds: u32,
) -> Result<(), StopOverlayError> {
    let app = Application::builder()
        .application_id("com.apexshot.recording.countdown")
        .build();

    app.connect_activate(move |application| {
        let dim_windows = setup_dim_windows(application, params);
        setup_countdown_window(application, params, seconds, dim_windows);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

pub fn run_recording_stop_overlay(
    stop_tx: oneshot::Sender<StopAction>,
) -> Result<(), StopOverlayError> {
    run_recording_controls(
        RecordingControlsParams {
            capture_x: 0,
            capture_y: 0,
            capture_w: 0,
            capture_h: 0,
            is_fullscreen: true,
            show_timer: false,
            use_shell_mask: false,
        },
        stop_tx,
    )
}

fn compute_bar_position(
    params: &RecordingControlsParams,
    screen_w: i32,
    screen_h: i32,
) -> (i32, i32) {
    if params.is_fullscreen {
        let x = (screen_w - BAR_W) / 2;
        let y = MARGIN;
        return (x, y);
    }

    let x = (params.capture_x + (params.capture_w - BAR_W) / 2)
        .clamp(MARGIN, (screen_w - BAR_W - MARGIN).max(MARGIN));

    let y_below = params.capture_y + params.capture_h + 12;
    if y_below + BAR_H + DOCK_SAFE <= screen_h {
        return (x, y_below);
    }

    let y_above = params.capture_y - BAR_H - 12;
    if y_above >= MARGIN {
        return (x, y_above);
    }

    ((screen_w - BAR_W) / 2, MARGIN)
}

fn setup_window(
    app: &Application,
    params: RecordingControlsParams,
    stop_tx: Arc<Mutex<Option<oneshot::Sender<StopAction>>>>,
    dim_windows: Vec<ApplicationWindow>,
) {
    install_controls_css();

    let (screen_w, screen_h) = display_size().unwrap_or((1920, 1080));
    let initial_pos = compute_bar_position(&params, screen_w, screen_h);
    let current_pos = Rc::new(Cell::new(initial_pos));
    let drag_start_pos = Rc::new(Cell::new(initial_pos));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recording")
        .default_width(BAR_W)
        .default_height(BAR_H)
        .decorated(false)
        .resizable(false)
        .build();

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();
    if layer_shell_active {
        let (pos_x, pos_y) = initial_pos;
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Bottom, false);
        window.set_anchor(Edge::Right, false);
        window.set_margin(Edge::Top, pos_y);
        window.set_margin(Edge::Left, pos_x);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        window.set_exclusive_zone(-1);
        window.set_namespace(Some("apexshot-recording-controls"));
    }

    if !layer_shell_active {
        let current_pos_realize = current_pos.clone();
        window.connect_realize(clone!(
            #[weak]
            window,
            move |_| {
                suppress_x11_controls_window_type(&window);
                let _ = request_x11_always_on_top(&window);
                let (x, y) = current_pos_realize.get();
                let _ = position_x11_window(&window, x, y);
            }
        ));

        let current_pos_map = current_pos.clone();
        window.connect_map(clone!(
            #[weak]
            window,
            move |_| {
                let (x, y) = current_pos_map.get();
                let _ = request_x11_always_on_top(&window);
                let _ = position_x11_window(&window, x, y);
            }
        ));
    }

    let stop_btn = icon_button_with_stop();
    let pause_btn = icon_button_with_pause();
    pause_btn.set_sensitive(false);

    let restart_btn = icon_button_with_restart();
    restart_btn.set_sensitive(false);

    let discard_btn = icon_button_with_discard();
    let menu_btn = icon_button_with_menu();

    let timer_label = Label::new(Some("0:00"));
    timer_label.add_css_class("rec-timer");

    let sep1 = create_separator();
    let sep2 = create_separator();
    let sep3 = create_separator();
    let sep4 = create_separator();

    let stop_container = GtkBox::new(Orientation::Horizontal, 0);
    stop_container.add_css_class("stop-container");
    stop_container.append(&stop_btn);
    if params.show_timer {
        stop_container.append(&timer_label);
    }
    
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("recording-controls-bar");
    bar.set_valign(gtk4::Align::Center);

    bar.append(&stop_container);
    bar.append(&sep1);
    bar.append(&pause_btn);
    bar.append(&sep2);
    bar.append(&restart_btn);
    bar.append(&sep3);
    bar.append(&discard_btn);
    bar.append(&sep4);
    bar.append(&menu_btn);

    window.set_child(Some(&bar));

    if params.show_timer {
        let elapsed_secs = Rc::new(Cell::new(0u64));
        let elapsed_secs_timer = elapsed_secs.clone();
        let timer_label_weak = timer_label.downgrade();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let secs = elapsed_secs_timer.get() + 1;
            elapsed_secs_timer.set(secs);
            if let Some(lbl) = timer_label_weak.upgrade() {
                let mins = secs / 60;
                let s = secs % 60;
                lbl.set_text(&format!("{}:{:02}", mins, s));
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    let stop_tx_stop = stop_tx.clone();
    let window_weak = window.downgrade();
    let dim_window_weaks: Vec<_> = dim_windows.iter().map(|window| window.downgrade()).collect();
    stop_btn.connect_clicked(move |_| {
        send_stop_action(&stop_tx_stop, StopAction::Save);
        for dim_window in &dim_window_weaks {
            if let Some(window) = dim_window.upgrade() {
                window.close();
            }
        }
        if let Some(window) = window_weak.upgrade() {
            if let Some(app) = window.application() {
                app.quit();
            } else {
                window.close();
            }
        }
    });

    let stop_tx_discard = stop_tx.clone();
    let window_weak_discard = window.downgrade();
    let dim_window_weaks_discard: Vec<_> =
        dim_windows.iter().map(|window| window.downgrade()).collect();
    discard_btn.connect_clicked(move |_| {
        send_stop_action(&stop_tx_discard, StopAction::Discard);
        for dim_window in &dim_window_weaks_discard {
            if let Some(window) = dim_window.upgrade() {
                window.close();
            }
        }
        if let Some(window) = window_weak_discard.upgrade() {
            if let Some(app) = window.application() {
                app.quit();
            } else {
                window.close();
            }
        }
    });

    let stop_tx_esc = stop_tx.clone();
    let window_weak_esc = window.downgrade();
    let dim_window_weaks_esc: Vec<_> = dim_windows.iter().map(|window| window.downgrade()).collect();
    let key_ctrl = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        if key == Key::Escape {
            send_stop_action(&stop_tx_esc, StopAction::Save);
            for dim_window in &dim_window_weaks_esc {
                if let Some(window) = dim_window.upgrade() {
                    window.close();
                }
            }
            if let Some(window) = window_weak_esc.upgrade() {
                if let Some(app) = window.application() {
                    app.quit();
                } else {
                    window.close();
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    let drag = GestureDrag::new();
    drag.connect_drag_begin(clone!(
        #[strong]
        current_pos,
        #[strong]
        drag_start_pos,
        move |_, _, _| {
            drag_start_pos.set(current_pos.get());
        }
    ));
    drag.connect_drag_update(clone!(
        #[weak]
        window,
        #[strong]
        current_pos,
        #[strong]
        drag_start_pos,
        move |_, dx, dy| {
            let (start_x, start_y) = drag_start_pos.get();
            let next_x = (start_x + dx.round() as i32).clamp(0, (screen_w - BAR_W).max(0));
            let next_y = (start_y + dy.round() as i32).clamp(0, (screen_h - BAR_H).max(0));
            current_pos.set((next_x, next_y));
            if layer_shell_active {
                window.set_margin(Edge::Left, next_x);
                window.set_margin(Edge::Top, next_y);
            } else {
                let _ = position_x11_window(&window, next_x, next_y);
            }
        }
    ));
    bar.add_controller(drag);

    window.present();
}

fn setup_countdown_window(
    app: &Application,
    params: RecordingControlsParams,
    seconds: u32,
    dim_windows: Vec<ApplicationWindow>,
) {
    install_controls_css();

    let (screen_w, screen_h) = display_size().unwrap_or((1920, 1080));
    let initial_pos = compute_bar_position(&params, screen_w, screen_h);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recording Countdown")
        .default_width(BAR_W)
        .default_height(BAR_H)
        .decorated(false)
        .resizable(false)
        .focusable(false)
        .build();

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();
    if layer_shell_active {
        let (pos_x, pos_y) = initial_pos;
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_margin(Edge::Top, pos_y);
        window.set_margin(Edge::Left, pos_x);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(-1);
        window.set_namespace(Some("apexshot-recording-countdown"));
    } else {
        window.connect_realize(clone!(
            #[weak]
            window,
            move |_| {
                suppress_x11_controls_window_type(&window);
                let _ = request_x11_always_on_top(&window);
                let (x, y) = initial_pos;
                let _ = position_x11_window(&window, x, y);
            }
        ));

        window.connect_map(clone!(
            #[weak]
            window,
            move |_| {
                let (x, y) = initial_pos;
                let _ = request_x11_always_on_top(&window);
                let _ = position_x11_window(&window, x, y);
            }
        ));
    }

    let label = Label::new(Some(&seconds.to_string()));
    label.add_css_class("rec-timer");
    label.set_margin_end(14);

    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("recording-controls-bar");
    bar.set_valign(gtk4::Align::Center);

    let stop_btn = icon_button_with_stop();
    stop_btn.set_sensitive(false);

    let stop_container = GtkBox::new(Orientation::Horizontal, 0);
    stop_container.append(&stop_btn);
    stop_container.append(&label);
    bar.append(&stop_container);
    window.set_child(Some(&bar));

    let remaining = Rc::new(Cell::new(seconds));
    let remaining_tick = remaining.clone();
    let label_weak = label.downgrade();
    let window_weak = window.downgrade();
    let app_weak = app.downgrade();
    let dim_window_weaks: Vec<_> = dim_windows.iter().map(|window| window.downgrade()).collect();

    glib::timeout_add_local(Duration::from_secs(1), move || {
        let current = remaining_tick.get();
        if current <= 1 {
            for dim_window in &dim_window_weaks {
                if let Some(window) = dim_window.upgrade() {
                    window.close();
                }
            }
            if let Some(window) = window_weak.upgrade() {
                if let Some(app) = window.application() {
                    app.quit();
                } else {
                    window.close();
                }
            } else if let Some(app) = app_weak.upgrade() {
                app.quit();
            }
            return glib::ControlFlow::Break;
        }

        let next = current - 1;
        remaining_tick.set(next);
        if let Some(label) = label_weak.upgrade() {
            label.set_text(&next.to_string());
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });

    window.present();
}

fn setup_dim_windows(app: &Application, params: RecordingControlsParams) -> Vec<ApplicationWindow> {
    if params.use_shell_mask || params.is_fullscreen || params.capture_w <= 0 || params.capture_h <= 0 {
        return Vec::new();
    }

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if is_wayland && !gtk4_layer_shell::is_supported() {
        eprintln!(
            "[recording] Skipping area dim mask on Wayland without Layer Shell; regular GTK windows would occlude the desktop."
        );
        return Vec::new();
    }

    install_dim_css();

    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitor = monitor_for_capture(&display, &params);
    let Some(monitor) = monitor else {
        return Vec::new();
    };
    let geometry = monitor.geometry();
    let local_x = (params.capture_x - geometry.x()).clamp(0, geometry.width());
    let local_y = (params.capture_y - geometry.y()).clamp(0, geometry.height());
    let local_right = (params.capture_x + params.capture_w - geometry.x()).clamp(0, geometry.width());
    let local_bottom =
        (params.capture_y + params.capture_h - geometry.y()).clamp(0, geometry.height());

    let rects = vec![
        (0, 0, geometry.width(), local_y),
        (0, local_y, local_x, (local_bottom - local_y).max(0)),
        (
            local_right,
            local_y,
            (geometry.width() - local_right).max(0),
            (local_bottom - local_y).max(0),
        ),
        (0, local_bottom, geometry.width(), (geometry.height() - local_bottom).max(0)),
    ];

    let mut windows = Vec::new();
    for (x, y, width, height) in rects {
        if width <= 0 || height <= 0 {
            continue;
        }
        if let Some(window) = build_dim_window(app, &monitor, x, y, width, height) {
            windows.push(window);
        }
    }

    windows
}

fn build_dim_window(
    app: &Application,
    monitor: &gdk::Monitor,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<ApplicationWindow> {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recording Mask")
        .default_width(width)
        .default_height(height)
        .decorated(false)
        .resizable(false)
        .focusable(false)
        .build();

    window.set_can_focus(false);
    window.set_focusable(false);
    window.set_css_classes(&["recording-dim-overlay"]);

    let fill = DrawingArea::new();
    fill.set_content_width(width);
    fill.set_content_height(height);
    fill.set_draw_func(|_, cr, width, height| {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        cr.rectangle(0.0, 0.0, width as f64, height as f64);
        let _ = cr.fill();
    });
    window.set_child(Some(&fill));

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();
    if layer_shell_active {
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Bottom, false);
        window.set_anchor(Edge::Right, false);
        window.set_margin(Edge::Top, y);
        window.set_margin(Edge::Left, x);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_namespace(Some("apexshot-recording-mask"));
    } else {
        window.fullscreen_on_monitor(monitor);
        let x_pos = x;
        let y_pos = y;
        window.connect_realize(clone!(
            #[weak]
            window,
            move |_| {
                suppress_x11_controls_window_type(&window);
                let _ = request_x11_always_on_top(&window);
                let _ = position_x11_window(&window, x_pos, y_pos);
            }
        ));
        window.connect_map(clone!(
            #[weak]
            window,
            move |_| {
                let _ = request_x11_always_on_top(&window);
                let _ = position_x11_window(&window, x, y);
            }
        ));
    }

    window.present();
    Some(window)
}

fn install_controls_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".recording-controls-bar {
                background-color: #141414;
                border-radius: 12px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                padding: 4px 6px;
            }
            .stop-container {
                background-color: #1e1f22;
                border-radius: 8px;
                padding-left: 12px;
                padding-right: 16px;
            }
            .rec-btn {
                background: none;
                border: none;
                padding: 0;
                min-width: 52px;
                min-height: 52px;
                box-shadow: none;
                border-radius: 8px;
            }
            .rec-btn:hover {
                background-color: rgba(255, 255, 255, 0.08); /* 22 / 255 ≈ 0.08 */
            }
            .rec-btn:active {
                background-color: rgba(255, 255, 255, 0.12);
            }
            .rec-btn:disabled {
                opacity: 0.25;
            }
            .rec-timer {
                color: #f46357;
                font-size: 15pt;
                font-weight: 700;
                margin-left: 10px;
            }
            .rec-separator {
                background-color: rgba(255, 255, 255, 0.08);
                min-width: 1px;
                margin-top: 18px;
                margin-bottom: 18px;
            }",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn install_dim_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".recording-dim-overlay {
                background-color: transparent;
            }",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn icon_button_with_stop() -> Button {
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(40);
    drawing_area.set_content_height(40);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;

        cr.set_source_rgb(0.95, 0.38, 0.34); // f46357
        cr.set_line_width(1.8);
        cr.arc(cx, cy, 10.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        let sq = 4.0;
        cr.rectangle(cx - sq, cy - sq, sq * 2.0, sq * 2.0);
        let _ = cr.fill();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn icon_button_with_pause() -> Button {
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(40);
    drawing_area.set_content_height(40);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.8);
        cr.arc(cx, cy, 10.0, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        let bar_w = 2.2;
        let bar_h = 11.0;
        let gap = 3.5;
        cr.rectangle(cx - gap - bar_w/2.0, cy - bar_h / 2.0, bar_w, bar_h);
        cr.rectangle(cx + gap - bar_w/2.0, cy - bar_h / 2.0, bar_w, bar_h);
        let _ = cr.fill();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn icon_button_with_restart() -> Button {
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(40);
    drawing_area.set_content_height(40);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.8);
        
        // Incomplete circle arc
        let start_angle = 60.0 * std::f64::consts::PI / 180.0;
        let end_angle = 340.0 * std::f64::consts::PI / 180.0;
        cr.arc(cx, cy, 8.5, start_angle, end_angle);
        let _ = cr.stroke();

        // Arrow head at the end of the arc (start_angle)
        let head_r = 8.5;
        let tip_angle = start_angle;
        let base_angle1 = start_angle - 25.0 * std::f64::consts::PI / 180.0;

        cr.move_to(cx + head_r * tip_angle.cos(), cy - head_r * tip_angle.sin());
        cr.line_to(cx + (head_r + 4.0) * base_angle1.cos(), cy - (head_r + 4.0) * base_angle1.sin());
        cr.line_to(cx + (head_r - 4.0) * base_angle1.cos(), cy - (head_r - 4.0) * base_angle1.sin());
        cr.close_path();
        let _ = cr.fill();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn icon_button_with_discard() -> Button {
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(40);
    drawing_area.set_content_height(40);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.8);

        // Body
        let bw = 11.0;
        let bh = 13.0;
        let top = cy - 3.5;
        
        cr.set_line_join(cairo::LineJoin::Round);
        cr.rectangle(cx - bw/2.0, top, bw, bh);
        let _ = cr.stroke();

        // Lid
        cr.move_to(cx - 8.0, top - 1.5);
        cr.line_to(cx + 8.0, top - 1.5);
        let _ = cr.stroke();
        
        // Handle
        cr.rectangle(cx - 2.5, top - 4.0, 5.0, 2.5);
        let _ = cr.stroke();

        // Lines inside
        cr.move_to(cx - 2.0, top + 2.5);
        cr.line_to(cx - 2.0, top + 9.5);
        cr.move_to(cx + 2.0, top + 2.5);
        cr.line_to(cx + 2.0, top + 9.5);
        let _ = cr.stroke();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn icon_button_with_menu() -> Button {
    let drawing_area = DrawingArea::new();
    drawing_area.set_content_width(40);
    drawing_area.set_content_height(40);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;

        cr.set_source_rgb(0.63, 0.63, 0.65); // a0a0a5
        cr.set_line_width(1.8);
        cr.set_line_cap(cairo::LineCap::Round);

        let lw = 10.0;
        cr.move_to(cx - lw / 2.0, cy - 5.0);
        cr.line_to(cx + lw / 2.0, cy - 5.0);
        cr.move_to(cx - lw / 2.0, cy);
        cr.line_to(cx + lw / 2.0, cy);
        cr.move_to(cx - lw / 2.0, cy + 5.0);
        cr.line_to(cx + lw / 2.0, cy + 5.0);
        let _ = cr.stroke();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn create_separator() -> Separator {
    let sep = Separator::new(Orientation::Vertical);
    sep.add_css_class("rec-separator");
    sep
}

fn display_size() -> Option<(i32, i32)> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    let mut max_right = 0;
    let mut max_bottom = 0;
    for idx in 0..monitors.n_items() {
        let Some(item) = monitors.item(idx) else {
            continue;
        };
        let Ok(monitor) = item.downcast::<gdk::Monitor>() else {
            continue;
        };
        let geometry = monitor.geometry();
        max_right = max_right.max(geometry.x() + geometry.width());
        max_bottom = max_bottom.max(geometry.y() + geometry.height());
    }
    if max_right > 0 && max_bottom > 0 {
        Some((max_right, max_bottom))
    } else {
        None
    }
}

fn monitor_for_capture(display: &gdk::Display, params: &RecordingControlsParams) -> Option<gdk::Monitor> {
    let center_x = params.capture_x + params.capture_w / 2;
    let center_y = params.capture_y + params.capture_h / 2;
    let monitors = display.monitors();

    for idx in 0..monitors.n_items() {
        let Some(item) = monitors.item(idx) else {
            continue;
        };
        let Ok(monitor) = item.downcast::<gdk::Monitor>() else {
            continue;
        };
        let geometry = monitor.geometry();
        let inside_x = center_x >= geometry.x() && center_x < geometry.x() + geometry.width();
        let inside_y = center_y >= geometry.y() && center_y < geometry.y() + geometry.height();
        if inside_x && inside_y {
            return Some(monitor);
        }
    }

    None
}

fn send_stop_action(stop_tx: &Arc<Mutex<Option<oneshot::Sender<StopAction>>>>, action: StopAction) {
    if let Some(tx) = stop_tx.lock().ok().and_then(|mut guard| guard.take()) {
        let _ = tx.send(action);
    }
}

fn suppress_x11_controls_window_type(window: &impl IsA<gtk4::Window>) {
    let Some(surface) = window.as_ref().surface() else {
        return;
    };
    let Ok(x11_surface) = surface.downcast::<X11Surface>() else {
        return;
    };
    let Ok(xid) = u32::try_from(x11_surface.xid()) else {
        return;
    };
    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    if let (Ok(type_cookie), Ok(notif_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE"),
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_NOTIFICATION"),
    ) {
        if let (Ok(type_reply), Ok(notif_reply)) = (type_cookie.reply(), notif_cookie.reply()) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                type_reply.atom,
                xproto::AtomEnum::ATOM,
                &[notif_reply.atom],
            );
        }
    }

    if let Ok(cookie) = conn.intern_atom(false, b"_NET_WM_BYPASS_COMPOSITOR") {
        if let Ok(reply) = cookie.reply() {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                reply.atom,
                xproto::AtomEnum::CARDINAL,
                &[1u32],
            );
        }
    }

    let _ = conn.flush();
}

fn request_x11_always_on_top(window: &impl IsA<gtk4::Window>) -> Result<(), String> {
    let surface = window
        .as_ref()
        .surface()
        .ok_or_else(|| "missing GTK surface".to_string())?;
    let x11_surface = surface
        .downcast::<X11Surface>()
        .map_err(|_| "surface is not X11 (compositor does not expose X11 backend)".to_string())?;
    let xid = u32::try_from(x11_surface.xid())
        .map_err(|_| "X11 window id is out of range for xproto window type".to_string())?;
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn
        .setup()
        .roots
        .get(screen_num)
        .map(|screen| screen.root)
        .ok_or_else(|| "missing X11 root window".to_string())?;

    let net_wm_state = intern_atom(&conn, b"_NET_WM_STATE")?;
    let net_wm_state_above = intern_atom(&conn, b"_NET_WM_STATE_ABOVE")?;
    let net_wm_state_sticky = intern_atom(&conn, b"_NET_WM_STATE_STICKY")?;

    send_net_wm_state_client_message(&conn, root, xid, net_wm_state, 1, net_wm_state_above, 0)?;
    send_net_wm_state_client_message(&conn, root, xid, net_wm_state, 1, net_wm_state_sticky, 0)?;
    conn.configure_window(
        xid,
        &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn position_x11_window(window: &impl IsA<gtk4::Window>, x: i32, y: i32) -> Result<(), String> {
    let surface = window
        .as_ref()
        .surface()
        .ok_or_else(|| "missing GTK surface".to_string())?;
    let x11_surface = surface
        .downcast::<X11Surface>()
        .map_err(|_| "surface is not X11 (compositor does not expose X11 backend)".to_string())?;
    let xid = u32::try_from(x11_surface.xid())
        .map_err(|_| "X11 window id is out of range for xproto window type".to_string())?;
    let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    conn.configure_window(
        xid,
        &xproto::ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .stack_mode(xproto::StackMode::ABOVE),
    )
    .map_err(|e| e.to_string())?;
    conn.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn intern_atom<C: Connection>(conn: &C, atom_name: &[u8]) -> Result<u32, String> {
    conn.intern_atom(false, atom_name)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())
        .map(|reply| reply.atom)
}

fn send_net_wm_state_client_message<C: Connection>(
    conn: &C,
    root: xproto::Window,
    window: xproto::Window,
    net_wm_state_atom: u32,
    action: u32,
    first_property: u32,
    second_property: u32,
) -> Result<(), String> {
    let client_message = xproto::ClientMessageEvent::new(
        32,
        window,
        net_wm_state_atom,
        [action, first_property, second_property, 1, 0],
    );

    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        client_message,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
