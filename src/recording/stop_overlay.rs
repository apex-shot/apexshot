use gdk4x11::X11Surface;
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

const BAR_W: i32 = 380;
const BAR_H: i32 = 56;
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

#[derive(Debug, Clone, Copy)]
pub struct RecordingControlsParams {
    pub capture_x: i32,
    pub capture_y: i32,
    pub capture_w: i32,
    pub capture_h: i32,
    pub is_fullscreen: bool,
    pub show_timer: bool,
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
        setup_window(application, params, stop_tx_activate.clone());
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

    let restart_btn = text_button("↺");
    restart_btn.set_sensitive(false);

    let discard_btn = text_button("🗑");
    let menu_btn = text_button("≡");

    let timer_label = Label::new(Some("0:00"));
    timer_label.add_css_class("rec-timer");

    let sep = Separator::new(Orientation::Vertical);
    sep.add_css_class("rec-separator");

    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("recording-controls-bar");
    bar.set_margin_top(4);
    bar.set_margin_bottom(4);
    bar.set_margin_start(8);
    bar.set_margin_end(8);

    bar.append(&stop_btn);
    if params.show_timer {
        bar.append(&timer_label);
    }
    bar.append(&sep);
    bar.append(&pause_btn);
    bar.append(&restart_btn);
    bar.append(&discard_btn);
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
    stop_btn.connect_clicked(move |_| {
        send_stop_action(&stop_tx_stop, StopAction::Save);
        if let Some(window) = window_weak.upgrade() {
            window.close();
        }
    });

    let stop_tx_discard = stop_tx.clone();
    let window_weak_discard = window.downgrade();
    discard_btn.connect_clicked(move |_| {
        send_stop_action(&stop_tx_discard, StopAction::Discard);
        if let Some(window) = window_weak_discard.upgrade() {
            window.close();
        }
    });

    let stop_tx_esc = stop_tx.clone();
    let window_weak_esc = window.downgrade();
    let key_ctrl = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        if key == Key::Escape {
            send_stop_action(&stop_tx_esc, StopAction::Save);
            if let Some(window) = window_weak_esc.upgrade() {
                window.close();
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

fn install_controls_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            ".recording-controls-bar {                background-color: rgba(28, 28, 30, 0.92);                border-radius: 14px;                border: 1px solid rgba(255,255,255,0.08);            }            .rec-btn {                background: none;                border: none;                padding: 0;                min-width: 44px;                min-height: 44px;                box-shadow: none;            }            .rec-btn:hover {                background-color: rgba(255,255,255,0.10);                border-radius: 8px;            }            .rec-btn:disabled {                opacity: 0.35;            }            .rec-btn label {                color: rgba(255,255,255,0.85);                font-size: 18px;            }            .rec-timer {                color: rgba(220,60,60,1.0);                font-size: 15px;                font-weight: bold;                font-family: monospace;                margin-start: 4px;                margin-end: 10px;            }            .rec-separator {                background-color: rgba(255,255,255,0.15);                min-width: 1px;                margin-top: 12px;                margin-bottom: 12px;                margin-start: 4px;                margin-end: 4px;            }",
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
    drawing_area.set_content_width(36);
    drawing_area.set_content_height(36);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let radius = (w.min(h) as f64 / 2.0) - 3.0;

        cr.set_source_rgba(1.0, 0.22, 0.22, 1.0);
        cr.set_line_width(2.2);
        cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        let sq = radius * 0.42;
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
    drawing_area.set_content_width(36);
    drawing_area.set_content_height(36);
    drawing_area.set_draw_func(|_, cr, w, h| {
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let radius = (w.min(h) as f64 / 2.0) - 3.0;

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
        cr.set_line_width(2.0);
        cr.arc(cx, cy, radius, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        let bar_w = radius * 0.20;
        let bar_h = radius * 0.60;
        let gap = radius * 0.18;
        cr.rectangle(cx - gap - bar_w, cy - bar_h / 2.0, bar_w, bar_h);
        cr.rectangle(cx + gap, cy - bar_h / 2.0, bar_w, bar_h);
        let _ = cr.fill();
    });

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_child(Some(&drawing_area));
    button.add_css_class("rec-btn");
    button
}

fn text_button(text: &str) -> Button {
    let button = Button::with_label(text);
    button.set_has_frame(false);
    button.set_focusable(false);
    button.add_css_class("rec-btn");
    button
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
