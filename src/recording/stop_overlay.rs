use crate::overlay::webcam::start_webcam_preview;
use gdk4x11::X11Surface;
use gtk4::cairo;
use gtk4::gdk::{self, Key};
use gtk4::{
    glib::{self, clone},
    prelude::*,
    Application, ApplicationWindow, CssProvider, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureClick, GestureDrag,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::oneshot;
use x11rb::wrapper::ConnectionExt;
use x11rb::{
    connection::Connection,
    protocol::xproto::{self, ConnectionExt as _},
};

use crate::overlay::drawing::{draw_frosted_panel, rounded_rect_path};
use crate::overlay::layout::{RectF, ACTION_CARD_GAP, FEATURE_PANEL_MARGIN};
use crate::overlay::recording::layout::REC_ACTION_HEIGHT;

const BAR_PAD: f64 = 8.0;
const BAR_HEIGHT: f64 = REC_ACTION_HEIGHT + BAR_PAD * 2.0;
const STOP_CELL_W_WITH_TIMER: f64 = 120.0;
const STOP_CELL_W_ICON_ONLY: f64 = 72.0;
const ICON_CELL_W: f64 = 56.0;
const CELL_GAP: f64 = ACTION_CARD_GAP;
const CELL_H: f64 = REC_ACTION_HEIGHT;
const PANEL_RADIUS: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarTile {
    Stop,
    Pause,
    Restart,
    Discard,
}

fn compute_bar_width(show_timer: bool) -> f64 {
    let stop_w = if show_timer {
        STOP_CELL_W_WITH_TIMER
    } else {
        STOP_CELL_W_ICON_ONLY
    };
    BAR_PAD * 2.0 + stop_w + CELL_GAP * 3.0 + ICON_CELL_W * 3.0
}

fn bar_tile_rects(bar_x: f64, bar_y: f64, show_timer: bool) -> [(BarTile, RectF); 4] {
    let stop_w = if show_timer {
        STOP_CELL_W_WITH_TIMER
    } else {
        STOP_CELL_W_ICON_ONLY
    };
    let y = bar_y + BAR_PAD;
    let mut x = bar_x + BAR_PAD;
    let stop = RectF {
        x,
        y,
        width: stop_w,
        height: CELL_H,
    };
    x += stop_w + CELL_GAP;
    let pause = RectF {
        x,
        y,
        width: ICON_CELL_W,
        height: CELL_H,
    };
    x += ICON_CELL_W + CELL_GAP;
    let restart = RectF {
        x,
        y,
        width: ICON_CELL_W,
        height: CELL_H,
    };
    x += ICON_CELL_W + CELL_GAP;
    let discard = RectF {
        x,
        y,
        width: ICON_CELL_W,
        height: CELL_H,
    };
    [
        (BarTile::Stop, stop),
        (BarTile::Pause, pause),
        (BarTile::Restart, restart),
        (BarTile::Discard, discard),
    ]
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingControlsParams {
    pub capture_x: i32,
    pub capture_y: i32,
    pub capture_w: i32,
    pub capture_h: i32,
    pub is_fullscreen: bool,
    pub show_timer: bool,
    pub use_shell_mask: bool,
    pub show_webcam: bool,
    pub webcam_device: i32,
    pub webcam_size: usize,
    pub webcam_shape: usize,
    pub webcam_rel_x: f64,
    pub webcam_rel_y: f64,
    pub webcam_flip: bool,
    pub countdown_enabled: bool,
    pub countdown_seconds: u32,
    pub session_id: Option<String>,
}

impl Default for RecordingControlsParams {
    fn default() -> Self {
        Self {
            capture_x: 0,
            capture_y: 0,
            capture_w: 0,
            capture_h: 0,
            is_fullscreen: false,
            show_timer: true,
            use_shell_mask: false,
            show_webcam: false,
            webcam_device: -1,
            webcam_size: 1,
            webcam_shape: 3,
            webcam_rel_x: 0.0,
            webcam_rel_y: 0.0,
            webcam_flip: false,
            countdown_enabled: false,
            countdown_seconds: 3,
            session_id: None,
        }
    }
}

pub fn run_recording_controls(
    params: RecordingControlsParams,
    session_id: Option<String>,
    _bus_name: Option<String>,
    stop_tx: oneshot::Sender<StopAction>,
) -> Result<(), StopOverlayError> {
    let stop_tx: Arc<Mutex<Option<oneshot::Sender<StopAction>>>> =
        Arc::new(Mutex::new(Some(stop_tx)));

    let _params_for_activate = RecordingControlsParams {
        capture_x: params.capture_x,
        capture_y: params.capture_y,
        capture_w: params.capture_w,
        capture_h: params.capture_h,
        is_fullscreen: params.is_fullscreen,
        show_timer: params.show_timer,
        use_shell_mask: params.use_shell_mask,
        show_webcam: params.show_webcam,
        webcam_device: params.webcam_device,
        webcam_size: params.webcam_size,
        webcam_shape: params.webcam_shape,
        webcam_rel_x: params.webcam_rel_x,
        webcam_rel_y: params.webcam_rel_y,
        webcam_flip: params.webcam_flip,
        countdown_enabled: params.countdown_enabled,
        countdown_seconds: params.countdown_seconds,
        session_id: session_id.clone(),
    };

    let app = Application::builder()
        .application_id("com.apexshot.recording")
        .build();

    let stop_tx_activate = stop_tx.clone();
    app.connect_activate(move |application| {
        let dim_windows = setup_dim_windows(application, params.clone());
        let _webcam_window = if params.show_webcam {
            setup_webcam_window(application, params.clone(), session_id.clone())
        } else {
            None
        };
        let controls_window = setup_window(
            application,
            params.clone(),
            stop_tx_activate.clone(),
            dim_windows,
            session_id.clone(),
        );
        controls_window.set_visible(true); // Show immediately for non-countdown mode
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

pub fn run_recording_countdown_bar(
    params: RecordingControlsParams,
    seconds: u32,
) -> Result<bool, StopOverlayError> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_activate = cancelled.clone();
    app.connect_activate(move |application| {
        let dim_windows = setup_dim_windows(application, params.clone());
        setup_countdown_window(
            application,
            params.clone(),
            seconds,
            dim_windows,
            cancelled_activate.clone(),
            false,
            None,
            None,
        );
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(!cancelled.load(Ordering::Relaxed))
}

/// Unified recording UI: dim windows + countdown + controls bar in one lifecycle.
/// Dim windows persist through the entire flow, eliminating surface transitions.
/// Prints "ready" to stdout when countdown finishes and controls are visible.
pub fn run_recording_ui(
    params: RecordingControlsParams,
    seconds: u32,
    stop_tx: oneshot::Sender<StopAction>,
) -> Result<(), StopOverlayError> {
    let stop_tx = Arc::new(Mutex::new(Some(stop_tx)));

    let app = Application::builder()
        .application_id("com.apexshot.recording")
        .build();

    app.connect_activate(move |application| {
        let dim_windows = setup_dim_windows(application, params.clone());
        let _webcam_window = if params.show_webcam {
            setup_webcam_window(application, params.clone(), params.session_id.clone())
        } else {
            None
        };
        let controls_window = setup_window(
            application,
            params.clone(),
            stop_tx.clone(),
            dim_windows.clone(),
            params.session_id.clone(),
        );
        setup_countdown_window(
            application,
            params.clone(),
            seconds,
            dim_windows,
            Arc::new(AtomicBool::new(false)),
            true,
            Some(controls_window),
            Some(stop_tx.clone()),
        );
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
            show_webcam: false,
            webcam_device: -1,
            webcam_size: 1,
            webcam_shape: 0,
            webcam_rel_x: 0.0,
            webcam_rel_y: 0.0,
            webcam_flip: false,
            countdown_enabled: false,
            countdown_seconds: 3,
            session_id: None,
        },
        None,
        None,
        stop_tx,
    )
}

fn compute_bar_position(
    params: &RecordingControlsParams,
    screen_w: i32,
    screen_h: i32,
) -> (i32, i32) {
    let bar_w = compute_bar_width(params.show_timer);
    let bar_h = BAR_HEIGHT;
    let margin = FEATURE_PANEL_MARGIN;
    let screen_w_f = screen_w as f64;
    let screen_h_f = screen_h as f64;

    if params.is_fullscreen || params.capture_w <= 0 || params.capture_h <= 0 {
        let x =
            ((screen_w_f - bar_w) / 2.0).clamp(margin, (screen_w_f - bar_w - margin).max(margin));
        return (x.round() as i32, margin.round() as i32);
    }

    let sel_x = params.capture_x as f64;
    let sel_y = params.capture_y as f64;
    let sel_w = params.capture_w as f64;
    let sel_h = params.capture_h as f64;

    let x =
        (sel_x + (sel_w - bar_w) / 2.0).clamp(margin, (screen_w_f - bar_w - margin).max(margin));

    let y =
        (sel_y + (sel_h - bar_h) / 2.0).clamp(margin, (screen_h_f - bar_h - margin).max(margin));

    (x.round() as i32, y.round() as i32)
}

fn setup_window(
    app: &Application,
    params: RecordingControlsParams,
    stop_tx: Arc<Mutex<Option<oneshot::Sender<StopAction>>>>,
    dim_windows: Vec<ApplicationWindow>,
    session_id: Option<String>,
) -> ApplicationWindow {
    install_controls_css();

    let display = gdk::Display::default().expect("No display");
    let monitor = monitor_for_capture(&display, &params);

    let (screen_w, screen_h) = display_size().unwrap_or((1920, 1080));
    let initial_pos = compute_bar_position(&params, screen_w, screen_h);

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();

    let (clamp_w, clamp_h) = if let Some(ref m) = monitor {
        let geom = m.geometry();
        (geom.width(), geom.height())
    } else {
        (screen_w, screen_h)
    };

    let initial_pos_mapped = if layer_shell_active {
        if let Some(ref m) = monitor {
            let geom = m.geometry();
            (initial_pos.0 - geom.x(), initial_pos.1 - geom.y())
        } else {
            initial_pos
        }
    } else {
        initial_pos
    };

    let current_pos = Rc::new(Cell::new(initial_pos_mapped));
    let drag_start_pos = Rc::new(Cell::new(initial_pos_mapped));

    let bar_w_f = compute_bar_width(params.show_timer);
    let bar_h_f = BAR_HEIGHT;
    let bar_w_i = bar_w_f.ceil() as i32;
    let bar_h_i = bar_h_f.ceil() as i32;

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recording")
        .default_width(bar_w_i)
        .default_height(bar_h_i)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("recording-controls-window");
    window.set_visible(false); // Hide during countdown, show after

    if layer_shell_active {
        let (pos_x, pos_y) = initial_pos_mapped;
        window.init_layer_shell();
        window.set_layer(Layer::Top);
        if let Some(ref m) = monitor {
            window.set_monitor(Some(m));
        }
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

    let elapsed_secs = Rc::new(Cell::new(0u64));
    let hovered_tile: Rc<Cell<Option<BarTile>>> = Rc::new(Cell::new(None));
    let is_paused: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let session_id_draw = session_id.clone();
    let show_timer = params.show_timer;

    let drawing_area = DrawingArea::new();
    drawing_area.add_css_class("recording-controls-canvas");
    drawing_area.set_content_width(bar_w_i);
    drawing_area.set_content_height(bar_h_i);

    {
        let elapsed = elapsed_secs.clone();
        let hovered = hovered_tile.clone();
        let paused = is_paused.clone();
        drawing_area.set_draw_func(move |_, cr, width, height| {
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            let w = width as f64;
            let h = height as f64;
            draw_frosted_panel(cr, 0.0, 0.0, w, h, PANEL_RADIUS, w, h, None);

            let tiles = bar_tile_rects(0.0, 0.0, show_timer);
            let hover = hovered.get();
            let secs = elapsed.get();

            for (tile, rect) in tiles.iter() {
                let is_stop = *tile == BarTile::Stop;
                let is_hovered = hover == Some(*tile);
                let is_disabled = session_id_draw.is_none();

                if is_stop {
                    draw_primary_pill(cr, *rect, is_hovered);
                } else {
                    draw_control_cell(cr, *tile, *rect, is_hovered, is_disabled, paused.get());
                }

                let alpha = if is_disabled {
                    0.32
                } else if is_hovered {
                    1.0
                } else {
                    0.94
                };

                match tile {
                    BarTile::Stop => draw_stop_glyph(cr, *rect, show_timer, secs),
                    BarTile::Pause => {
                        if paused.get() {
                            draw_resume_glyph(cr, *rect, alpha)
                        } else {
                            draw_pause_glyph(cr, *rect, alpha)
                        }
                    }
                    BarTile::Restart => draw_restart_glyph(cr, *rect, alpha),
                    BarTile::Discard => draw_discard_glyph(cr, *rect, alpha),
                }
            }
        });
    }

    window.set_child(Some(&drawing_area));

    if params.show_timer {
        let elapsed_for_timer = elapsed_secs.clone();
        let drawing_area_weak = drawing_area.downgrade();
        let window_weak = window.downgrade();
        let paused_for_timer = is_paused.clone();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(window) = window_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            // In countdown mode the controls window is created hidden before the
            // actual recorder starts. Do not count that waiting time as recording
            // time; begin ticking only once the controls are shown.
            if !window.is_visible() {
                return glib::ControlFlow::Continue;
            }
            if paused_for_timer.get() {
                return glib::ControlFlow::Continue;
            }
            elapsed_for_timer.set(elapsed_for_timer.get() + 1);
            if let Some(area) = drawing_area_weak.upgrade() {
                area.queue_draw();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    let motion = EventControllerMotion::new();
    {
        let hovered = hovered_tile.clone();
        let drawing_area_weak = drawing_area.downgrade();
        motion.connect_motion(move |_, x, y| {
            let tiles = bar_tile_rects(0.0, 0.0, show_timer);
            let mut hit: Option<BarTile> = None;
            for (tile, rect) in tiles.iter() {
                if rect.contains(x, y) {
                    hit = Some(*tile);
                    break;
                }
            }
            if hovered.replace(hit) != hit {
                if let Some(area) = drawing_area_weak.upgrade() {
                    area.queue_draw();
                }
            }
        });
    }
    {
        let hovered = hovered_tile.clone();
        let drawing_area_weak = drawing_area.downgrade();
        motion.connect_leave(move |_| {
            if hovered.replace(None).is_some() {
                if let Some(area) = drawing_area_weak.upgrade() {
                    area.queue_draw();
                }
            }
        });
    }
    drawing_area.add_controller(motion);

    let close_with_action = {
        let stop_tx = stop_tx.clone();
        let window_weak = window.downgrade();
        let dim_window_weaks: Vec<_> = dim_windows
            .iter()
            .map(|window| window.downgrade())
            .collect();
        Rc::new(move |action: StopAction| {
            send_stop_action(&stop_tx, action);
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
        })
    };

    let click = GestureClick::builder().button(1).build();
    {
        let close_with_action = close_with_action.clone();
        let session_id = session_id.clone();
        let paused = is_paused.clone();
        let drawing_area_weak = drawing_area.downgrade();
        let window_weak = window.downgrade();
        click.connect_released(move |gesture, n_press, x, y| {
            if n_press != 1 {
                return;
            }
            let tiles = bar_tile_rects(0.0, 0.0, show_timer);
            for (tile, rect) in tiles.iter() {
                if rect.contains(x, y) {
                    gesture.set_state(gtk4::EventSequenceState::Claimed);
                    match tile {
                        BarTile::Stop => close_with_action(StopAction::Save),
                        BarTile::Discard => close_with_action(StopAction::Discard),
                        BarTile::Pause => {
                            if session_id.is_some() {
                                let currently_paused = paused.get();
                                if currently_paused {
                                    paused.set(false);
                                    println!("resume");
                                } else {
                                    paused.set(true);
                                    println!("pause");
                                }
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                                if let Some(area) = drawing_area_weak.upgrade() {
                                    area.queue_draw();
                                }
                            }
                        }
                        BarTile::Restart => {
                            if session_id.is_some() {
                                println!("restart");
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                                if let Some(window) = window_weak.upgrade() {
                                    if let Some(app) = window.application() {
                                        app.quit();
                                    } else {
                                        window.close();
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
            }
        });
    }
    drawing_area.add_controller(click);

    let key_ctrl = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();
    {
        let close_with_action = close_with_action.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            if key == Key::Escape {
                close_with_action(StopAction::Save);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
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
            let next_x = (start_x + dx.round() as i32).clamp(0, (clamp_w - bar_w_i).max(0));
            let next_y = (start_y + dy.round() as i32).clamp(0, (clamp_h - bar_h_i).max(0));
            current_pos.set((next_x, next_y));
            if layer_shell_active {
                window.set_margin(Edge::Left, next_x);
                window.set_margin(Edge::Top, next_y);
            } else {
                let _ = position_x11_window(&window, next_x, next_y);
            }
        }
    ));
    drawing_area.add_controller(drag);

    window
}

fn setup_countdown_window(
    app: &Application,
    params: RecordingControlsParams,
    seconds: u32,
    dim_windows: Vec<ApplicationWindow>,
    cancelled: Arc<AtomicBool>,
    unified_mode: bool,
    controls_window: Option<ApplicationWindow>,
    cancel_stop_tx: Option<Arc<Mutex<Option<oneshot::Sender<StopAction>>>>>,
) {
    let display = gdk::Display::default().expect("No display");
    let monitor = monitor_for_capture(&display, &params);
    let monitor_geom = monitor.as_ref().map(|m| m.geometry());

    let (win_w, win_h) = if let Some(ref geom) = monitor_geom {
        (geom.width(), geom.height())
    } else {
        display_size().unwrap_or((1920, 1080))
    };

    let countdown_center = if !params.is_fullscreen && params.capture_w > 0 && params.capture_h > 0
    {
        let global_center_x = params.capture_x as f64 + params.capture_w as f64 / 2.0;
        let global_center_y = params.capture_y as f64 + params.capture_h as f64 / 2.0;
        if let Some(ref geom) = monitor_geom {
            (
                global_center_x - geom.x() as f64,
                global_center_y - geom.y() as f64,
            )
        } else {
            (global_center_x, global_center_y)
        }
    } else {
        (win_w as f64 / 2.0, win_h as f64 / 2.0)
    };

    install_countdown_css();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Recording Countdown")
        .default_width(win_w)
        .default_height(win_h)
        .decorated(false)
        .resizable(false)
        .focusable(false)
        .build();
    window.set_css_classes(&["recording-countdown-overlay"]);

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();
    if layer_shell_active {
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        if let Some(ref m) = monitor {
            window.set_monitor(Some(m));
        }
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Right, true);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(-1);
        window.set_namespace(Some("apexshot-recording-countdown"));
    } else {
        if let Some(ref m) = monitor {
            window.fullscreen_on_monitor(m);
        } else {
            window.fullscreen();
        }
        window.connect_realize(clone!(
            #[weak]
            window,
            move |_| {
                suppress_x11_controls_window_type(&window);
                let _ = request_x11_always_on_top(&window);
            }
        ));
    }

    let remaining = Rc::new(Cell::new(seconds.max(1) as i32));
    let hovered = Rc::new(Cell::new(false));
    let drawing_area = DrawingArea::new();
    drawing_area.set_css_classes(&["recording-countdown-canvas"]);
    drawing_area.set_content_width(win_w);
    drawing_area.set_content_height(win_h);
    drawing_area.set_draw_func({
        let remaining = remaining.clone();
        let hovered = hovered.clone();
        move |_, cr, width, height| {
            cr.set_operator(cairo::Operator::Source);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Over);

            let bubble_size = 184.0;
            let center_x = countdown_center
                .0
                .clamp(bubble_size / 2.0, width as f64 - bubble_size / 2.0);
            let center_y = countdown_center
                .1
                .clamp(bubble_size / 2.0, height as f64 - bubble_size / 2.0);
            let bubble_x = center_x - bubble_size / 2.0;
            let bubble_y = center_y - bubble_size / 2.0;
            let is_hovered = hovered.get();

            cr.set_source_rgba(
                if is_hovered { 132.0 / 255.0 } else { 0.0 },
                if is_hovered { 38.0 / 255.0 } else { 0.0 },
                if is_hovered { 24.0 / 255.0 } else { 0.0 },
                if is_hovered {
                    242.0 / 255.0
                } else {
                    240.0 / 255.0
                },
            );
            cr.arc(
                bubble_x + bubble_size / 2.0,
                bubble_y + bubble_size / 2.0,
                bubble_size / 2.0,
                0.0,
                std::f64::consts::TAU,
            );
            let _ = cr.fill();

            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(if is_hovered { 34.0 } else { 72.0 });
            if is_hovered {
                cr.set_source_rgb(1.0, 228.0 / 255.0, 214.0 / 255.0);
            } else {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            }
            let text = if is_hovered {
                "Cancel".to_string()
            } else {
                remaining.get().to_string()
            };
            if let Ok(extents) = cr.text_extents(&text) {
                cr.move_to(
                    bubble_x + (bubble_size - extents.width()) / 2.0 - extents.x_bearing(),
                    bubble_y + (bubble_size - extents.height()) / 2.0 - extents.y_bearing(),
                );
                let _ = cr.show_text(&text);
            }
        }
    });

    let motion = gtk4::EventControllerMotion::new();
    motion.connect_motion({
        let hovered = hovered.clone();
        let drawing_area = drawing_area.clone();
        move |_, x, y| {
            let bubble_size = 184.0;
            let cx = countdown_center
                .0
                .clamp(bubble_size / 2.0, win_w as f64 - bubble_size / 2.0);
            let cy = countdown_center
                .1
                .clamp(bubble_size / 2.0, win_h as f64 - bubble_size / 2.0);
            let inside = (x - cx).hypot(y - cy) <= bubble_size / 2.0;
            if hovered.replace(inside) != inside {
                drawing_area.queue_draw();
            }
        }
    });
    motion.connect_leave({
        let hovered = hovered.clone();
        let drawing_area = drawing_area.clone();
        move |_| {
            if hovered.replace(false) {
                drawing_area.queue_draw();
            }
        }
    });
    drawing_area.add_controller(motion);

    let click = gtk4::GestureClick::builder().button(1).build();
    click.connect_pressed({
        let app_weak = app.downgrade();
        let window_weak = window.downgrade();
        let cancel_stop_tx = cancel_stop_tx.clone();
        move |_, _, x, y| {
            let bubble_size = 184.0;
            let cx = countdown_center
                .0
                .clamp(bubble_size / 2.0, win_w as f64 - bubble_size / 2.0);
            let cy = countdown_center
                .1
                .clamp(bubble_size / 2.0, win_h as f64 - bubble_size / 2.0);
            if (x - cx).hypot(y - cy) <= bubble_size / 2.0 {
                cancelled.store(true, Ordering::Relaxed);
                if let Some(tx) = cancel_stop_tx
                    .as_ref()
                    .and_then(|tx| tx.lock().ok().and_then(|mut guard| guard.take()))
                {
                    let _ = tx.send(StopAction::Discard);
                }
                if unified_mode {
                    println!("discard");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
                if let Some(window) = window_weak.upgrade() {
                    window.close();
                }
                if let Some(app) = app_weak.upgrade() {
                    app.quit();
                }
            }
        }
    });
    drawing_area.add_controller(click);

    window.set_child(Some(&drawing_area));

    let drawing_area_weak = drawing_area.downgrade();
    let window_weak = window.downgrade();
    let app_weak = app.downgrade();
    let dim_window_weaks: Vec<_> = dim_windows
        .iter()
        .map(|window| window.downgrade())
        .collect();

    glib::timeout_add_local(Duration::from_secs(1), move || {
        let next = remaining.get() - 1;
        if next <= 0 {
            if !unified_mode {
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
            } else {
                if let Some(window) = window_weak.upgrade() {
                    window.close();
                }
                // Show controls window after countdown finishes
                if let Some(controls) = controls_window.as_ref() {
                    controls.set_visible(true);
                }
                println!("ready");
                {
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }
            return glib::ControlFlow::Break;
        }
        remaining.set(next);
        if let Some(area) = drawing_area_weak.upgrade() {
            area.queue_draw();
        }
        glib::ControlFlow::Continue
    });

    window.present();
}

fn setup_dim_windows(app: &Application, params: RecordingControlsParams) -> Vec<ApplicationWindow> {
    if params.use_shell_mask
        || params.is_fullscreen
        || params.capture_w <= 0
        || params.capture_h <= 0
    {
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
    // Coordinates from overlay are already monitor-local, no need to subtract geometry
    let local_x = params.capture_x.clamp(0, geometry.width());
    let local_y = params.capture_y.clamp(0, geometry.height());
    let local_right = (params.capture_x + params.capture_w).clamp(0, geometry.width());
    let local_bottom = (params.capture_y + params.capture_h).clamp(0, geometry.height());

    let rects = vec![
        (0, 0, geometry.width(), local_y),
        (0, local_y, local_x, (local_bottom - local_y).max(0)),
        (
            local_right,
            local_y,
            (geometry.width() - local_right).max(0),
            (local_bottom - local_y).max(0),
        ),
        (
            0,
            local_bottom,
            geometry.width(),
            (geometry.height() - local_bottom).max(0),
        ),
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
        window.set_monitor(Some(monitor));
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Bottom, false);
        window.set_anchor(Edge::Right, false);
        window.set_margin(Edge::Top, y);
        window.set_margin(Edge::Left, x);
        window.set_keyboard_mode(KeyboardMode::None);
        // Critical on Hyprland: keep mask coordinates relative to the physical
        // output, not the compositor workarea below reserved bars/panels.
        window.set_exclusive_zone(-1);
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

fn install_countdown_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".recording-countdown-overlay,
             .recording-countdown-overlay > contents,
             .recording-countdown-canvas {
                background: transparent;
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

fn install_controls_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".recording-controls-window,
             .recording-controls-window > contents,
             .recording-controls-canvas {
                background: transparent;
                background-color: transparent;
            }
            .countdown-bar-container {
                background-color: #141414;
                border-radius: 12px;
                padding: 10px;
                border: 1px solid rgba(255, 255, 255, 0.15);
            }
            .countdown-label {
                color: white;
                font-size: 24pt;
                font-weight: 800;
                margin-bottom: 8px;
            }
            .countdown-progress {
                background-color: rgba(255, 255, 255, 0.1);
                border-radius: 4px;
                min-height: 8px;
            }
            .countdown-progress-inner {
                background: linear-gradient(to right, #f46357, #ff9b8a);
                border-radius: 4px;
                min-height: 8px;
            }
            .webcam-window {
                border-radius: 1000px; /* Overridden for square */
                overflow: hidden;
                box-shadow: 0 8px 32px rgba(0,0,0,0.5);
                border: 2px solid rgba(255, 255, 255, 0.2);
            }
            .recording-webcam-window,
            .recording-webcam-window > contents,
            .recording-webcam-canvas {
                background: transparent;
                background-color: transparent;
            }
            .webcam-picture {
                background-color: #000;
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

fn draw_control_cell(
    cr: &cairo::Context,
    tile: BarTile,
    rect: RectF,
    hovered: bool,
    disabled: bool,
    paused: bool,
) {
    let active_pause = matches!(tile, BarTile::Pause) && paused;
    if !hovered && !active_pause {
        return;
    }

    let (r, g, b, fill_alpha, stroke_alpha) = if disabled {
        (1.0, 1.0, 1.0, 0.04, 0.06)
    } else {
        match tile {
            BarTile::Pause if active_pause => {
                (74.0 / 255.0, 144.0 / 255.0, 226.0 / 255.0, 0.24, 0.34)
            }
            BarTile::Pause => (74.0 / 255.0, 144.0 / 255.0, 226.0 / 255.0, 0.18, 0.26),
            BarTile::Restart => (1.0, 184.0 / 255.0, 77.0 / 255.0, 0.18, 0.28),
            BarTile::Discard => (235.0 / 255.0, 87.0 / 255.0, 87.0 / 255.0, 0.18, 0.30),
            BarTile::Stop => (1.0, 1.0, 1.0, 0.09, 0.14),
        }
    };

    rounded_rect_path(cr, rect.x, rect.y, rect.width, rect.height, 10.0);
    cr.set_source_rgba(r, g, b, fill_alpha);
    let _ = cr.fill();

    rounded_rect_path(
        cr,
        rect.x + 0.75,
        rect.y + 0.75,
        rect.width - 1.5,
        rect.height - 1.5,
        9.25,
    );
    cr.set_source_rgba(r, g, b, stroke_alpha);
    cr.set_line_width(1.2);
    let _ = cr.stroke();
}

fn draw_primary_pill(cr: &cairo::Context, rect: RectF, hovered: bool) {
    if hovered {
        rounded_rect_path(cr, rect.x, rect.y, rect.width, rect.height, 10.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.09);
        let _ = cr.fill();
    }
    let (path_x, path_y, path_w, path_h) = (
        rect.x + 3.0,
        rect.y + 3.0,
        rect.width - 6.0,
        rect.height - 6.0,
    );
    rounded_rect_path(cr, path_x, path_y, path_w, path_h, 9.0);
    cr.set_source_rgba(176.0 / 255.0, 92.0 / 255.0, 56.0 / 255.0, 88.0 / 255.0);
    let _ = cr.fill();

    let _ = cr.save();
    rounded_rect_path(cr, path_x, path_y, path_w, path_h, 9.0);
    cr.clip();
    rounded_rect_path(
        cr,
        rect.x + 3.8,
        rect.y + 3.8,
        rect.width - 7.6,
        rect.height - 7.6,
        8.4,
    );
    cr.set_source_rgba(1.0, 212.0 / 255.0, 178.0 / 255.0, 152.0 / 255.0);
    cr.set_line_width(1.1);
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn draw_stop_glyph(cr: &cairo::Context, rect: RectF, show_timer: bool, secs: u64) {
    let cy = rect.y + rect.height / 2.0;
    let icon_cx = if show_timer {
        rect.x + 22.0
    } else {
        rect.x + rect.width / 2.0
    };

    let shadow_alpha = 0.32;
    cr.set_source_rgba(0.0, 0.0, 0.0, shadow_alpha);
    cr.set_line_width(1.8);
    cr.arc(icon_cx + 0.6, cy + 0.8, 10.0, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    let sq = 4.0;
    cr.rectangle(icon_cx + 0.6 - sq, cy + 0.8 - sq, sq * 2.0, sq * 2.0);
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.set_line_width(1.8);
    cr.arc(icon_cx, cy, 10.0, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    cr.rectangle(icon_cx - sq, cy - sq, sq * 2.0, sq * 2.0);
    let _ = cr.fill();

    if show_timer {
        let mins = secs / 60;
        let s = secs % 60;
        let text = format!("{}:{:02}", mins, s);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(16.5);
        let label_x = rect.x + 42.0;
        let label_y = cy + 5.6;

        cr.set_source_rgba(0.0, 0.0, 0.0, shadow_alpha);
        cr.move_to(label_x + 0.6, label_y + 0.8);
        let _ = cr.show_text(&text);
        cr.set_source_rgba(1.0, 232.0 / 255.0, 214.0 / 255.0, 1.0);
        cr.move_to(label_x, label_y);
        let _ = cr.show_text(&text);
    }
}

fn draw_glyph_with_shadow<F>(cr: &cairo::Context, rect: RectF, alpha: f64, draw: F)
where
    F: Fn(&cairo::Context, f64, f64),
{
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;

    cr.set_source_rgba(0.0, 0.0, 0.0, (alpha * 0.40).min(0.44));
    cr.set_line_width(1.8);
    draw(cr, cx + 0.6, cy + 0.8);

    cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
    cr.set_line_width(1.8);
    draw(cr, cx, cy);
}

fn draw_pause_glyph(cr: &cairo::Context, rect: RectF, alpha: f64) {
    draw_glyph_with_shadow(cr, rect, alpha, |cr, cx, cy| {
        cr.arc(cx, cy, 10.0, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        let bar_w = 2.2;
        let bar_h = 11.0;
        let gap = 3.5;
        cr.rectangle(cx - gap - bar_w / 2.0, cy - bar_h / 2.0, bar_w, bar_h);
        cr.rectangle(cx + gap - bar_w / 2.0, cy - bar_h / 2.0, bar_w, bar_h);
        let _ = cr.fill();
    });
}

fn draw_resume_glyph(cr: &cairo::Context, rect: RectF, alpha: f64) {
    draw_glyph_with_shadow(cr, rect, alpha, |cr, cx, cy| {
        cr.arc(cx, cy, 10.0, 0.0, 2.0 * PI);
        let _ = cr.stroke();
        let triangle_size = 5.0;
        cr.move_to(cx - triangle_size / 2.0, cy - triangle_size);
        cr.line_to(cx - triangle_size / 2.0, cy + triangle_size);
        cr.line_to(cx + triangle_size, cy);
        cr.close_path();
        let _ = cr.fill();
    });
}

fn draw_restart_glyph(cr: &cairo::Context, rect: RectF, alpha: f64) {
    draw_glyph_with_shadow(cr, rect, alpha, |cr, cx, cy| {
        let start_angle = 60.0_f64.to_radians();
        let end_angle = 340.0_f64.to_radians();
        cr.arc(cx, cy, 8.5, start_angle, end_angle);
        let _ = cr.stroke();

        let head_r = 8.5;
        let tip_angle = start_angle;
        let base_angle1 = start_angle - 25.0_f64.to_radians();
        cr.move_to(cx + head_r * tip_angle.cos(), cy - head_r * tip_angle.sin());
        cr.line_to(
            cx + (head_r + 4.0) * base_angle1.cos(),
            cy - (head_r + 4.0) * base_angle1.sin(),
        );
        cr.line_to(
            cx + (head_r - 4.0) * base_angle1.cos(),
            cy - (head_r - 4.0) * base_angle1.sin(),
        );
        cr.close_path();
        let _ = cr.fill();
    });
}

fn draw_discard_glyph(cr: &cairo::Context, rect: RectF, alpha: f64) {
    draw_glyph_with_shadow(cr, rect, alpha, |cr, cx, cy| {
        cr.set_line_join(cairo::LineJoin::Round);
        let bw = 11.0;
        let bh = 13.0;
        let top = cy - 3.5;
        cr.rectangle(cx - bw / 2.0, top, bw, bh);
        let _ = cr.stroke();
        cr.move_to(cx - 8.0, top - 1.5);
        cr.line_to(cx + 8.0, top - 1.5);
        let _ = cr.stroke();
        cr.rectangle(cx - 2.5, top - 4.0, 5.0, 2.5);
        let _ = cr.stroke();
        cr.move_to(cx - 2.0, top + 2.5);
        cr.line_to(cx - 2.0, top + 9.5);
        cr.move_to(cx + 2.0, top + 2.5);
        cr.line_to(cx + 2.0, top + 9.5);
        let _ = cr.stroke();
    });
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

fn monitor_for_capture(
    display: &gdk::Display,
    params: &RecordingControlsParams,
) -> Option<gdk::Monitor> {
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

fn rounded_rect_path_local(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

fn recording_webcam_size(
    params: &RecordingControlsParams,
    screen_w: i32,
    screen_h: i32,
) -> (i32, i32) {
    const MARGIN: i32 = 10;
    let bounds_w = if params.is_fullscreen || params.capture_w <= 0 {
        screen_w
    } else {
        params.capture_w
    };
    let bounds_h = if params.is_fullscreen || params.capture_h <= 0 {
        screen_h
    } else {
        params.capture_h
    };

    let (mut width, mut height) = match params.webcam_size {
        0 => (120, 160),
        2 => (280, 370),
        3 => (360, 480),
        4 => (
            (bounds_w - 2 * MARGIN).max(1),
            (bounds_h - 2 * MARGIN).max(1),
        ),
        _ => (200, 260),
    };

    match params.webcam_shape {
        0 | 1 => height = width,
        2 => height = ((width as f64) * 0.75).round() as i32,
        _ => {}
    }

    width = width.min((bounds_w - 2 * MARGIN).max(1));
    height = height.min((bounds_h - 2 * MARGIN).max(1));
    (width, height)
}

fn setup_webcam_window(
    application: &Application,
    params: RecordingControlsParams,
    _session_id: Option<String>,
) -> Option<ApplicationWindow> {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("ApexShot Webcam")
        .default_width(240)
        .default_height(240)
        .decorated(false)
        .resizable(false)
        .focusable(false)
        .build();

    window.add_css_class("recording-webcam-window");

    let (screen_w, screen_h) = display_size().unwrap_or((1920, 1080));
    let display = gdk::Display::default();
    let monitor = display
        .as_ref()
        .and_then(|display| monitor_for_capture(display, &params));
    let monitor_geom = monitor.as_ref().map(|m| m.geometry());
    let (mut webcam_w, mut webcam_h) = recording_webcam_size(&params, screen_w, screen_h);
    webcam_w = webcam_w.max(1);
    webcam_h = webcam_h.max(1);
    window.set_default_size(webcam_w, webcam_h);

    let bounds_x = if params.is_fullscreen {
        0
    } else {
        params.capture_x
    };
    let bounds_y = if params.is_fullscreen {
        0
    } else {
        params.capture_y
    };
    let bounds_w = if params.is_fullscreen || params.capture_w <= 0 {
        screen_w
    } else {
        params.capture_w
    };
    let bounds_h = if params.is_fullscreen || params.capture_h <= 0 {
        screen_h
    } else {
        params.capture_h
    };
    let max_x = (bounds_x + bounds_w - webcam_w).max(bounds_x);
    let max_y = (bounds_y + bounds_h - webcam_h).max(bounds_y);
    let x = (bounds_x as f64 + (max_x - bounds_x) as f64 * params.webcam_rel_x.clamp(0.0, 1.0))
        .round() as i32;
    let y = (bounds_y as f64
        + (max_y - bounds_y) as f64 * (1.0 - params.webcam_rel_y.clamp(0.0, 1.0)))
    .round() as i32;

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let layer_shell_active = is_wayland && gtk4_layer_shell::is_supported();

    if layer_shell_active {
        let output_origin_x = monitor_geom.as_ref().map(|g| g.x()).unwrap_or(0);
        let output_origin_y = monitor_geom.as_ref().map(|g| g.y()).unwrap_or(0);
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        if let Some(ref monitor) = monitor {
            window.set_monitor(Some(monitor));
        }
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Bottom, false);
        window.set_anchor(Edge::Right, false);
        window.set_margin(Edge::Top, bounds_y - output_origin_y);
        window.set_margin(Edge::Left, bounds_x - output_origin_x);
        window.set_default_size(bounds_w, bounds_h);
        window.set_opacity(1.0);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(-1);
        window.set_namespace(Some("apexshot-webcam"));
    } else {
        window.connect_realize(clone!(
            #[weak]
            window,
            move |_| {
                suppress_x11_controls_window_type(&window);
                let _ = request_x11_always_on_top(&window);
                let _ = position_x11_window(&window, x, y);
            }
        ));
    }

    if params.webcam_shape == 1 {
        window.add_css_class("webcam-circle");
        let provider = CssProvider::new();
        provider.load_from_data(".webcam-circle { border-radius: 9999px; overflow: hidden; }");
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("No display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let webcam_preview = if params.webcam_device >= 0 {
        start_webcam_preview(params.webcam_device, params.webcam_flip)
    } else {
        None
    };

    let drawing_area = DrawingArea::new();
    drawing_area.set_css_classes(&["recording-webcam-canvas"]);
    drawing_area.set_content_width(webcam_w);
    drawing_area.set_content_height(webcam_h);
    if layer_shell_active {
        drawing_area.set_halign(gtk4::Align::Start);
        drawing_area.set_valign(gtk4::Align::Start);
        drawing_area.set_margin_start(x - bounds_x);
        drawing_area.set_margin_top(y - bounds_y);
    }

    if let Some(preview) = webcam_preview.as_ref() {
        let frames = preview.frame_handle();
        drawing_area.set_draw_func(move |_, cr, w, h| {
            let radius = (w as f64 / 2.0).min(h as f64 / 2.0);
            cr.save().ok();
            if params.webcam_shape == 0 {
                cr.arc(
                    w as f64 / 2.0,
                    h as f64 / 2.0,
                    radius,
                    0.0,
                    std::f64::consts::TAU,
                );
                cr.clip();
            } else {
                let r = if params.webcam_shape == 1 { 8.0 } else { 12.0 };
                rounded_rect_path_local(cr, 0.0, 0.0, w as f64, h as f64, r);
                cr.clip();
            }

            let frame = frames.lock().ok().and_then(|slot| slot.clone());
            if let Some(frame) = frame {
                if let Ok(surface) = cairo::ImageSurface::create_for_data(
                    frame.bgra,
                    cairo::Format::ARgb32,
                    frame.width,
                    frame.height,
                    frame.width * 4,
                ) {
                    cr.scale(
                        w as f64 / frame.width as f64,
                        h as f64 / frame.height as f64,
                    );
                    let _ = cr.set_source_surface(&surface, 0.0, 0.0);
                    let _ = cr.paint();
                }
            } else {
                cr.set_source_rgb(0.05, 0.05, 0.05);
                let _ = cr.paint();
            }
            cr.restore().ok();
        });
        let area_weak = drawing_area.downgrade();
        glib::timeout_add_local(Duration::from_millis(33), move || {
            if let Some(area) = area_weak.upgrade() {
                area.queue_draw();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    } else {
        drawing_area.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(0.05, 0.05, 0.05);
            if params.webcam_shape == 0 {
                cr.arc(
                    w as f64 / 2.0,
                    h as f64 / 2.0,
                    (w as f64 / 2.0).min(h as f64 / 2.0),
                    0.0,
                    2.0 * std::f64::consts::PI,
                );
            } else {
                let r = if params.webcam_shape == 1 { 8.0 } else { 12.0 };
                rounded_rect_path_local(cr, 0.0, 0.0, w as f64, h as f64, r);
            }
            let _ = cr.fill();

            cr.set_source_rgb(0.3, 0.3, 0.3);
            cr.set_line_width(2.0);
            let cx = w as f64 / 2.0;
            let cy = h as f64 / 2.0;
            cr.arc(cx, cy - 10.0, 15.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();
            cr.move_to(cx - 20.0, cy + 20.0);
            cr.line_to(cx + 20.0, cy + 20.0);
            let _ = cr.stroke();
        });
    }
    window.set_child(Some(&drawing_area));

    let drag = GestureDrag::new();
    let current_x = Rc::new(Cell::new(x));
    let current_y = Rc::new(Cell::new(y));
    let drag_start_x = Rc::new(Cell::new(x));
    let drag_start_y = Rc::new(Cell::new(y));

    drag.connect_drag_begin(clone!(
        #[strong]
        current_x,
        #[strong]
        current_y,
        #[strong]
        drag_start_x,
        #[strong]
        drag_start_y,
        move |_, _, _| {
            drag_start_x.set(current_x.get());
            drag_start_y.set(current_y.get());
        }
    ));

    drag.connect_drag_update(clone!(
        #[weak]
        window,
        #[weak]
        drawing_area,
        #[strong]
        current_x,
        #[strong]
        current_y,
        #[strong]
        drag_start_x,
        #[strong]
        drag_start_y,
        move |_, dx, dy| {
            let next_x = (drag_start_x.get() + dx as i32).clamp(bounds_x, max_x);
            let next_y = (drag_start_y.get() + dy as i32).clamp(bounds_y, max_y);
            current_x.set(next_x);
            current_y.set(next_y);
            if layer_shell_active {
                drawing_area.set_margin_start(next_x - bounds_x);
                drawing_area.set_margin_top(next_y - bounds_y);
            } else {
                let _ = position_x11_window(&window, next_x, next_y);
            }
        }
    ));

    drag.connect_drag_end(clone!(
        #[strong]
        current_x,
        #[strong]
        current_y,
        move |_, _, _| {
            let rel_x = if max_x > bounds_x {
                (current_x.get() - bounds_x) as f64 / (max_x - bounds_x) as f64
            } else {
                0.0
            };
            let rel_y = if max_y > bounds_y {
                1.0 - ((current_y.get() - bounds_y) as f64 / (max_y - bounds_y) as f64)
            } else {
                0.0
            };
            let exe =
                std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
            let _ = std::process::Command::new(exe)
                .arg("recording-control")
                .arg("move-webcam")
                .arg(rel_x.to_string())
                .arg(rel_y.to_string())
                .spawn();
        }
    ));
    window.add_controller(drag);

    if let Some(preview) = webcam_preview {
        window.connect_destroy(move |_| {
            let _keep_preview_alive = &preview;
        });
    }

    window.present();
    Some(window)
}
