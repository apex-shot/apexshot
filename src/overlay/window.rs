use super::api::{OverlaySelection, SelectionError, SelectionResult};
use super::background::BackgroundFrame;
use super::drawing::{draw_overlay, CLICK_COLORS_LEN};
use super::geometry::{
    clamp_point_to_bounds, current_selection_rect, cursor_name_for_handle, detect_resize_handle,
    is_inside_selection, selection_area_from_state, set_selection_rect, update_selection_for_drag,
    SelectionRectF,
};
use super::hit_testing::{
    capture_crop_menu_contains, capture_crop_menu_hit_item, toolbar_hit_at, toolbar_item_at,
};
use super::icons::{
    ToolbarIcon, TOOLBAR_AREA_INDEX, TOOLBAR_FULLSCREEN_INDEX, TOOLBAR_ICONS,
    TOOLBAR_RECORDING_INDEX, TOOLBAR_SCROLL_INDEX, TOOLBAR_WINDOW_INDEX,
};
use super::layout::{
    RectF, ToolbarHit, DEFAULT_SELECTION_HEIGHT, DEFAULT_SELECTION_WIDTH, MIN_SELECTION_HEIGHT,
    MIN_SELECTION_WIDTH,
};
use super::recording::hit_testing::{
    click_dropdown_hit_item, click_options_hit_item, click_options_menu_contains,
    recording_crop_menu_contains, recording_crop_menu_hit_item, recording_tile_at,
    settings_menu_contains, settings_menu_hit_item, webcam_options_hit_item,
    webcam_options_menu_contains,
};
use super::recording::layout::{compute_dropdown_popup_y, RecordPanelTile};
use super::recording::state::{OverlayIntent, SettingsTab};
use super::state::{DragMode, OverlayMode, SelectorState};
use super::webcam::{first_webcam_device, start_webcam_preview};
use crate::capture_overlay::{RecordingRequest, RecordingType};
use gtk4::gdk::Key;
use gtk4::{
    gdk,
    glib::{self, clone},
    prelude::*,
    Application, ApplicationWindow, CssProvider, EventControllerKey, EventControllerMotion,
    GestureClick, GestureDrag,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::sync::{Arc, Mutex};
use x11rb::wrapper::ConnectionExt;

fn recording_request_from_state(
    st: &SelectorState,
    record_type: RecordingType,
) -> RecordingRequest {
    let area = current_selection_rect(st);
    RecordingRequest {
        x: area.left.round() as i32,
        y: area.top.round() as i32,
        width: area.width().round() as i32,
        height: area.height().round() as i32,
        record_type,
        controls: st.recording.rec_controls,
        mic: st.recording.mic_toggle,
        speaker: st.recording.speaker_toggle,
        clicks: st.recording.rec_clicks,
        keystrokes: st.recording.rec_keystrokes,
        webcam: st.recording.rec_webcam,
        click_size: st.recording.click_size,
        click_color: st.recording.click_color as u8,
        click_style: st.recording.click_style as u8,
        click_animate: st.recording.click_animate,
        webcam_size: st.recording.webcam_size as u8,
        webcam_shape: st.recording.webcam_shape as u8,
        webcam_flip: st.recording.webcam_flip,
        webcam_device: st.recording.webcam_device,
        webcam_rel_x: st.recording.webcam_rel_x,
        webcam_rel_y: st.recording.webcam_rel_y,
        display_rec_time: st.recording.display_rec_time,
        hidpi: st.recording.hidpi,
        notifications: st.recording.do_not_disturb,
        cursor: st.recording.show_cursor,
        remember_selection: st.recording.remember_selection,
        dim_screen: st.recording.dim_screen,
        countdown: st.recording.show_countdown,
        video_max_res: st.recording.video_max_res as u8,
        video_fps: st.recording.video_fps as u8,
        record_mono: st.recording.record_mono,
        open_editor: st.recording.open_editor,
        gif_fps: st.recording.gif_fps.round().clamp(5.0, 60.0) as u8,
        gif_quality: st.recording.gif_quality,
        gif_size_idx: st.recording.gif_size_idx as u8,
        optimize_gif: st.recording.optimize_gif,
        fullscreen: st.fullscreen_mode,
        ..RecordingRequest::default()
    }
}

fn poll_daemon_audio_levels() -> Option<(f64, f64)> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        crate::daemon::DAEMON_BUS_NAME,
        crate::daemon::DAEMON_OBJECT_PATH,
        crate::daemon::DAEMON_INTERFACE,
    )
    .ok()?;
    let mic = proxy.call::<_, _, f64>("GetMicLevel", &()).ok()?;
    let speaker = proxy.call::<_, _, f64>("GetSpeakerLevel", &()).ok()?;
    Some((mic.clamp(0.0, 1.0), speaker.clamp(0.0, 1.0)))
}

fn sync_webcam_preview(st: &mut SelectorState) {
    if !st.recording.rec_webcam || st.recording.webcam_device < 0 {
        st.recording.webcam_preview = None;
        st.recording.webcam_frame = None;
        return;
    }
    if st.recording.webcam_preview.is_none() {
        if let Some(preview) =
            start_webcam_preview(st.recording.webcam_device, st.recording.webcam_flip)
        {
            st.recording.webcam_frame = Some(preview.frame_handle());
            st.recording.webcam_preview = Some(preview);
        }
    }
}

pub(crate) fn send_selection_result(
    state: &Arc<Mutex<SelectorState>>,
    result_tx: &std::sync::mpsc::Sender<SelectionResult>,
    window: &ApplicationWindow,
    screen_width: i32,
    screen_height: i32,
    background: Option<&BackgroundFrame>,
) {
    let st = state.lock().unwrap();
    let area = selection_area_from_state(&st, screen_width, screen_height, background);
    let intent = st.intent; // Read intent to determine result type (TODO: wire up different result types)
    drop(st);

    // TODO: Based on intent, emit different result types (RecordingRequest, OcrRequested, etc.)
    let _ = intent;

    let result = if area.is_valid() {
        Ok(OverlaySelection::Area(Some(area)))
    } else {
        Ok(OverlaySelection::Area(None))
    };
    let _ = result_tx.send(result);
    window.close();
}

pub(crate) fn install_overlay_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            window.overlay {
                background-color: transparent;
                transition: none;
                transition-duration: 0s;
                animation: none;
                animation-duration: 0s;
            }

            window.overlay > * {
                background-color: transparent;
            }

            drawingarea {
                background-color: transparent;
            }
            ",
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

/// On X11, tell the compositor to treat this window as a transient system
/// overlay (no open/close animation, no taskbar entry, no pager entry).
///
/// This is called from `connect_realize` — i.e. the XID exists but the
/// window has not been mapped yet — so the compositor sees all hints on
/// the very first MapNotify and never starts an animation.
pub(crate) fn suppress_x11_compositor_animation(window: &ApplicationWindow) {
    use gdk4x11::X11Surface;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{self, ConnectionExt as _},
    };

    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(x11_surface) = surface.downcast::<X11Surface>() else {
        return; // Wayland – nothing to do
    };
    let Ok(xid) = u32::try_from(x11_surface.xid()) else {
        return;
    };
    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    // _NET_WM_BYPASS_COMPOSITOR = 1
    // Asks the compositor to skip compositing this window entirely, which
    // also disables any open/close transition effects.
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

    // _NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_UTILITY
    // UTILITY windows are never animated by compositors (Mutter, KWin, Picom).
    // We prefer UTILITY over SPLASH because SPLASH can cause focus/stacking
    // issues on some window managers.
    if let (Ok(type_cookie), Ok(util_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE"),
        conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_UTILITY"),
    ) {
        if let (Ok(type_reply), Ok(util_reply)) = (type_cookie.reply(), util_cookie.reply()) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                type_reply.atom,
                xproto::AtomEnum::ATOM,
                &[util_reply.atom],
            );
        }
    }

    // _NET_WM_STATE: add SKIP_TASKBAR + SKIP_PAGER so the overlay never
    // appears in the taskbar or workspace switcher.
    if let (Ok(state_cookie), Ok(skip_taskbar_cookie), Ok(skip_pager_cookie)) = (
        conn.intern_atom(false, b"_NET_WM_STATE"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR"),
        conn.intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER"),
    ) {
        if let (Ok(state_reply), Ok(skip_taskbar_reply), Ok(skip_pager_reply)) = (
            state_cookie.reply(),
            skip_taskbar_cookie.reply(),
            skip_pager_cookie.reply(),
        ) {
            let _ = conn.change_property32(
                xproto::PropMode::REPLACE,
                xid,
                state_reply.atom,
                xproto::AtomEnum::ATOM,
                &[skip_taskbar_reply.atom, skip_pager_reply.atom],
            );
        }
    }

    let _ = conn.flush();
}

fn webcam_preview_size(
    sel_w: f64,
    sel_h: f64,
    webcam_size: usize,
    webcam_shape: usize,
) -> (f64, f64) {
    const MARGIN: f64 = 10.0;
    let (mut preview_w, mut preview_h) = match webcam_size {
        0 => (120.0, 160.0),
        2 => (280.0, 370.0),
        3 => (360.0, 480.0),
        4 => (
            (sel_w - 2.0 * MARGIN).max(1.0),
            (sel_h - 2.0 * MARGIN).max(1.0),
        ),
        _ => (200.0, 260.0),
    };

    match webcam_shape {
        0 | 1 => preview_h = preview_w,
        2 => preview_h = preview_w * 0.75,
        _ => {}
    }

    preview_w = preview_w.min((sel_w - 2.0 * MARGIN).max(1.0));
    preview_h = preview_h.min((sel_h - 2.0 * MARGIN).max(1.0));
    (preview_w, preview_h)
}

fn webcam_preview_rect(st: &SelectorState, selection: SelectionRectF) -> RectF {
    const MARGIN: f64 = 10.0;
    let sel_w = selection.width();
    let sel_h = selection.height();
    let (preview_w, preview_h) = webcam_preview_size(
        sel_w,
        sel_h,
        st.recording.webcam_size,
        st.recording.webcam_shape,
    );
    let min_x = selection.left + MARGIN;
    let max_x = min_x.max(selection.left + sel_w - preview_w - MARGIN);
    let min_y = selection.top + MARGIN;
    let max_y = min_y.max(selection.top + sel_h - preview_h - MARGIN);

    RectF {
        x: min_x + (max_x - min_x) * st.recording.webcam_rel_x.clamp(0.0, 1.0),
        y: min_y + (max_y - min_y) * (1.0 - st.recording.webcam_rel_y.clamp(0.0, 1.0)),
        width: preview_w,
        height: preview_h,
    }
}

fn set_webcam_preview_top_left(st: &mut SelectorState, selection: SelectionRectF, x: f64, y: f64) {
    const MARGIN: f64 = 10.0;
    let sel_w = selection.width();
    let sel_h = selection.height();
    let (preview_w, preview_h) = webcam_preview_size(
        sel_w,
        sel_h,
        st.recording.webcam_size,
        st.recording.webcam_shape,
    );
    let min_x = selection.left + MARGIN;
    let max_x = min_x.max(selection.left + sel_w - preview_w - MARGIN);
    let min_y = selection.top + MARGIN;
    let max_y = min_y.max(selection.top + sel_h - preview_h - MARGIN);
    let clamped_x = x.clamp(min_x, max_x);
    let clamped_y = y.clamp(min_y, max_y);

    st.recording.webcam_rel_x = if max_x > min_x {
        (clamped_x - min_x) / (max_x - min_x)
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    st.recording.webcam_rel_y = if max_y > min_y {
        1.0 - ((clamped_y - min_y) / (max_y - min_y))
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
}

fn aspect_ratio_for_index(index: usize) -> f64 {
    const RATIOS: &[f64] = &[
        0.0,
        1.0,
        5.0 / 4.0,
        4.0 / 3.0,
        7.0 / 5.0,
        3.0 / 2.0,
        16.0 / 10.0,
        16.0 / 9.0,
        2.35,
        2.0 / 3.0,
        9.0 / 16.0,
    ];
    RATIOS.get(index).copied().unwrap_or(0.0)
}

fn active_aspect_ratio(st: &SelectorState) -> f64 {
    if st.recording.panel_open {
        aspect_ratio_for_index(st.recording.record_aspect_ratio_index)
    } else {
        aspect_ratio_for_index(st.capture_aspect_ratio_index)
    }
}

fn apply_aspect_to_selection(
    st: &mut SelectorState,
    ratio: f64,
    bounds_width: f64,
    bounds_height: f64,
) {
    if ratio <= 0.0 || !st.completed {
        return;
    }

    let sel = current_selection_rect(st);
    let mut new_w = sel.width();
    let mut new_h = new_w / ratio;
    if new_h > sel.height() {
        new_h = sel.height();
        new_w = new_h * ratio;
    }

    new_w = new_w.clamp(MIN_SELECTION_WIDTH, bounds_width.max(MIN_SELECTION_WIDTH));
    new_h = new_h.clamp(
        MIN_SELECTION_HEIGHT,
        bounds_height.max(MIN_SELECTION_HEIGHT),
    );
    if new_w / ratio > bounds_height {
        new_h = bounds_height;
        new_w = new_h * ratio;
    }
    if new_h * ratio > bounds_width {
        new_w = bounds_width;
        new_h = new_w / ratio;
    }

    let center_x = (sel.left + sel.right) / 2.0;
    let center_y = (sel.top + sel.bottom) / 2.0;
    let width = new_w.max(MIN_SELECTION_WIDTH).round();
    let height = new_h.max(MIN_SELECTION_HEIGHT).round();
    let left = (center_x - width / 2.0).clamp(0.0, (bounds_width - width).max(0.0));
    let top = (center_y - height / 2.0).clamp(0.0, (bounds_height - height).max(0.0));

    set_selection_rect(
        st,
        SelectionRectF {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
    );
    st.completed = true;
}

pub(crate) fn setup_window(
    app: &Application,
    state: Arc<Mutex<SelectorState>>,
    result_tx: std::sync::mpsc::Sender<SelectionResult>,
    background: Option<BackgroundFrame>,
) {
    // Suppress GTK-side animations so the overlay appears/disappears instantly.
    install_overlay_css();

    // Get the display and monitor for screen dimensions
    let display = match gdk::Display::default() {
        Some(d) => d,
        None => {
            let _ = result_tx.send(Err(SelectionError::InitError("No display found".into())));
            return;
        }
    };

    // Get screen dimensions from the first monitor
    let monitor = {
        let monitors = display.monitors();
        let n = monitors.n_items();
        if n == 0 {
            let _ = result_tx.send(Err(SelectionError::InitError("No monitor found".into())));
            return;
        }
        // Get the first monitor from the list model
        match monitors.item(0) {
            Some(obj) => match obj.downcast::<gdk::Monitor>() {
                Ok(m) => m,
                Err(_) => {
                    let _ = result_tx.send(Err(SelectionError::InitError(
                        "Failed to get monitor".into(),
                    )));
                    return;
                }
            },
            None => {
                let _ = result_tx.send(Err(SelectionError::InitError(
                    "No monitor at index 0".into(),
                )));
                return;
            }
        }
    };

    let geometry = monitor.geometry();
    let screen_width = geometry.width();
    let screen_height = geometry.height();

    // Create the window
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(screen_width)
        .default_height(screen_height)
        .decorated(false)
        .resizable(false)
        .css_classes(["overlay", "transparent"])
        .build();

    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    // On Wayland, layer-shell gives a true transparent overlay surface.
    // Without this, some compositors show a black backing surface.
    let wayland_layer_shell = is_wayland && gtk4_layer_shell::is_supported();

    // NOTE: We no longer bail out when background.is_none() && Wayland-without-layer-shell.
    // Instead we fall through to window.set_fullscreened(true) which works on GNOME Wayland.
    // The drawing code already handles background=None by painting a dark semi-transparent
    // overlay — this is the "capture after selection" (Option B) path.

    if wayland_layer_shell {
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Left, true);
        window.set_anchor(Edge::Right, true);
        let keyboard_mode = if state
            .lock()
            .map(|st| st.overlay_mode == OverlayMode::CrosshairCapture)
            .unwrap_or(false)
        {
            // Hyprland stops compositor global binds while a layer-shell surface
            // has exclusive keyboard focus. Crosshair mode is easy to leave open
            // accidentally, so avoid making all ApexShot shortcuts appear dead
            // until the app/overlay is restarted.
            KeyboardMode::OnDemand
        } else {
            KeyboardMode::Exclusive
        };
        window.set_keyboard_mode(keyboard_mode);
        window.set_monitor(Some(&monitor));
        window.set_namespace(Some("apexshot-area-selector"));
        window.set_exclusive_zone(-1);
    } else {
        // X11 or Wayland-without-layer-shell (e.g. GNOME Wayland):
        // Use a regular fullscreen window. The compositor will grant it
        // focus via the XDG activation token embedded in DESKTOP_STARTUP_ID.
        window.set_fullscreened(true);
        window.set_decorated(false);
    }

    // Get the surface for cursor control
    let surface = window.surface();

    // Set cursor to crosshair when hovering over the window
    if let Some(ref surface) = surface {
        let cursor = gdk::Cursor::from_name("crosshair", None);
        surface.set_cursor(cursor.as_ref());
    }

    // Create a drawing area for rendering the selection
    let drawing_area = gtk4::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();

    let state_draw = state.clone();
    let background_draw = background.clone();
    drawing_area.set_draw_func(move |_, context, width, height| {
        draw_overlay(
            context,
            width,
            height,
            &state_draw,
            background_draw.as_ref(),
        );
    });

    {
        let mut st = state.lock().unwrap();
        let screen_width_f = screen_width.max(1) as f64;
        let screen_height_f = screen_height.max(1) as f64;
        if st.overlay_mode == OverlayMode::CrosshairCapture {
            st.start_x = screen_width_f / 2.0;
            st.start_y = screen_height_f / 2.0;
            st.current_x = st.start_x;
            st.current_y = st.start_y;
            st.completed = false;
        } else {
            let initial_width = DEFAULT_SELECTION_WIDTH
                .min(screen_width_f)
                .max(MIN_SELECTION_WIDTH.min(screen_width_f));
            let initial_height = DEFAULT_SELECTION_HEIGHT
                .min(screen_height_f)
                .max(MIN_SELECTION_HEIGHT.min(screen_height_f));
            let initial_left = ((screen_width_f - initial_width) / 2.0).max(0.0);
            let initial_top = ((screen_height_f - initial_height) / 2.0).max(0.0);

            st.start_x = initial_left;
            st.start_y = initial_top;
            st.current_x = initial_left + initial_width;
            st.current_y = initial_top + initial_height;
            st.completed = true;
        }
        st.cancelled = false;
        st.is_dragging = false;
    }

    // Set the drawing area as the child
    window.set_child(Some(&drawing_area));

    let state_webcam_tick = state.clone();
    drawing_area.add_tick_callback(move |area, _| {
        if state_webcam_tick
            .lock()
            .map(|st| st.recording.rec_webcam && st.recording.webcam_preview.is_some())
            .unwrap_or(false)
        {
            area.queue_draw();
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Continue
        }
    });

    let audio_levels = Arc::new(Mutex::new((0.0_f64, 0.0_f64)));
    {
        let audio_levels = audio_levels.clone();
        std::thread::spawn(move || loop {
            if let Some(levels) = poll_daemon_audio_levels() {
                if let Ok(mut guard) = audio_levels.lock() {
                    *guard = levels;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        });
    }

    let state_audio_tick = state.clone();
    let drawing_area_weak_audio = drawing_area.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let (mic_level, speaker_level) = audio_levels
            .lock()
            .map(|guard| *guard)
            .unwrap_or((0.0, 0.0));
        if let Ok(mut st) = state_audio_tick.lock() {
            if !st.recording.panel_open {
                return glib::ControlFlow::Continue;
            }
            let old_mic = st.recording.mic_level;
            let old_speaker = st.recording.speaker_level;
            st.recording.mic_level = if st.recording.mic_toggle {
                mic_level
            } else {
                0.0
            };
            st.recording.speaker_level = if st.recording.speaker_toggle {
                speaker_level
            } else {
                0.0
            };
            if (old_mic - st.recording.mic_level).abs() > 0.01
                || (old_speaker - st.recording.speaker_level).abs() > 0.01
            {
                if let Some(area) = drawing_area_weak_audio.upgrade() {
                    area.queue_draw();
                }
            }
        }
        glib::ControlFlow::Continue
    });

    let motion_controller = EventControllerMotion::new();
    let state_motion = state.clone();
    let drawing_area_weak_motion = drawing_area.downgrade();
    let window_weak_motion = window.downgrade();
    motion_controller.connect_motion(move |_, x, y| {
        let (cursor_name, hover_changed, _done) = {
            let mut st = state_motion.lock().unwrap();
            if st.overlay_mode == OverlayMode::CrosshairCapture {
                let (x, y) = clamp_point_to_bounds(x, y, screen_width as f64, screen_height as f64);
                st.current_x = x;
                st.current_y = y;
                drop(st);
                if let Some(da) = drawing_area_weak_motion.upgrade() {
                    da.queue_draw();
                }
                ("crosshair".to_string(), false, true)
            } else {
                let rect = current_selection_rect(&st);

                // GIF slider dragging — update value from X position
                if let Some(slider) = st.recording.gif_slider_dragging {
                    if st.recording.settings_menu_open {
                        let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                            .clamp(10.0, screen_width as f64 - 450.0);
                        let value_x = menu_x + 130.0;
                        if slider == 0 {
                            let slider_x = value_x + 55.0;
                            let slider_w = 220.0;
                            let click_x = x.clamp(slider_x, slider_x + slider_w);
                            st.recording.gif_fps = 5.0 + (click_x - slider_x) / slider_w * 55.0;
                        } else {
                            let q_slider_w = 160.0;
                            let click_x = x.clamp(value_x, value_x + q_slider_w);
                            st.recording.gif_quality =
                                ((click_x - value_x) / q_slider_w).clamp(0.0, 1.0);
                        }
                    }
                    st.recording.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_motion.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }

                // Click slider dragging
                if st.recording.click_slider_dragging && st.recording.click_options_open {
                    let rect = current_selection_rect(&st);
                    let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                        .clamp(10.0, screen_width as f64 - 450.0);
                    let value_x = menu_x + 130.0;
                    let slider_x = value_x;
                    let slider_w = 280.0;
                    let click_x = x.clamp(slider_x, slider_x + slider_w);
                    st.recording.click_size = ((click_x - slider_x) / slider_w).clamp(0.0, 1.0);
                    st.recording.hovered_click_item = -1;
                    st.recording.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_motion.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }

                // Capture crop menu hover check
                if st.capture_crop_menu_open {
                    let item = capture_crop_menu_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.map(|i| i as i32).unwrap_or(-1);
                    let changed = next != st.hovered_capture_crop_menu_item;
                    if changed {
                        st.hovered_capture_crop_menu_item = next;
                    }
                    st.recording.hovered_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;
                    // Clear other hovers
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.recording.crop_menu_open {
                    let item = recording_crop_menu_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.map(|i| i as i32).unwrap_or(-1);
                    let changed = next != st.recording.hovered_crop_menu_item;
                    if changed {
                        st.recording.hovered_crop_menu_item = next;
                    }
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.recording.settings_menu_open {
                    if st.recording.settings_dropdown_open.is_some() {
                        st.recording.hovered_settings_item = -1;
                        st.hovered_capture_crop_menu_item = -1;
                        st.recording.hovered_crop_menu_item = -1;
                        st.hover_tool_index = None;
                        st.hover_size_panel = false;
                        st.hover_crop_panel = false;
                        st.recording.hover_record_tile = None;
                        ("pointer".to_string(), false, true)
                    } else {
                        let item = settings_menu_hit_item(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                            st.recording.settings_tab,
                        );
                        let next = item.unwrap_or(-1);
                        let changed = next != st.recording.hovered_settings_item;
                        if changed {
                            st.recording.hovered_settings_item = next;
                        }
                        st.hovered_capture_crop_menu_item = -1;
                        st.recording.hovered_crop_menu_item = -1;
                        st.hover_tool_index = None;
                        st.hover_size_panel = false;
                        st.hover_crop_panel = false;
                        st.recording.hover_record_tile = None;
                        ("pointer".to_string(), changed, true)
                    }
                } else if st.recording.click_options_open {
                    let item = click_options_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.unwrap_or(-1);
                    let changed = next != st.recording.hovered_click_item;
                    if changed {
                        st.recording.hovered_click_item = next;
                    }
                    st.recording.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.recording.webcam_options_open {
                    let item = webcam_options_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.unwrap_or(-1);
                    let changed = next != st.recording.hovered_webcam_item;
                    if changed {
                        st.recording.hovered_webcam_item = next;
                    }
                    st.recording.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.window_picker_open {
                    const POPUP_W_P: f64 = 320.0;
                    const ITEM_H_P: f64 = 28.0;
                    const HEADER_H_P: f64 = 30.0;
                    const PAD_P: f64 = 8.0;
                    let n = st.windows.len();
                    let popup_h = PAD_P * 2.0 + HEADER_H_P + n as f64 * ITEM_H_P;
                    let (center_x, center_y) = if st.completed || st.is_dragging {
                        let r = current_selection_rect(&st);
                        (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
                    } else {
                        (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
                    };
                    let popup_x = (center_x - POPUP_W_P / 2.0)
                        .clamp(10.0, (screen_width as f64 - POPUP_W_P - 10.0).max(10.0));
                    let popup_y = (center_y - popup_h / 2.0)
                        .clamp(10.0, (screen_height as f64 - popup_h - 10.0).max(10.0));
                    let list_y = popup_y + PAD_P + HEADER_H_P;

                    let mut next_entry = -1;
                    if x >= popup_x && x <= popup_x + POPUP_W_P && y >= list_y {
                        let idx = ((y - list_y) / ITEM_H_P) as i32;
                        if idx >= 0 && (idx as usize) < st.windows.len() {
                            next_entry = idx;
                        }
                    }

                    let changed = st.hovered_window_picker_entry != next_entry;
                    st.hovered_window_picker_entry = next_entry;
                    st.hovered_scroll_popup_close = false;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.scroll_popup_open {
                    // Scroll popup hover handling
                    let (cx, cy) = if st.completed || st.is_dragging {
                        let r = current_selection_rect(&st);
                        (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
                    } else {
                        (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
                    };
                    let popup_w = 360.0;
                    let popup_h = 170.0;
                    let popup_x = cx - popup_w / 2.0;
                    let popup_y = cy - popup_h / 2.0;
                    let close_size = 22.0;
                    let close_x = popup_x + popup_w - close_size - 10.0;
                    let close_y = popup_y + 10.0;

                    let next_hover_close = x >= close_x
                        && x <= close_x + close_size
                        && y >= close_y
                        && y <= close_y + close_size;
                    let changed = st.hovered_scroll_popup_close != next_hover_close;
                    if changed {
                        st.hovered_scroll_popup_close = next_hover_close;
                    }
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), true, true)
                } else {
                    let record_hit = if st.recording.panel_open {
                        recording_tile_at(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                        )
                    } else {
                        None
                    };
                    let hit = if st.recording.panel_open {
                        None
                    } else {
                        toolbar_hit_at(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                        )
                    };

                    let mut next_hovered_window = None;
                    if !st.completed && !st.is_dragging && hit.is_none() && record_hit.is_none() {
                        for (i, win) in st.windows.iter().enumerate() {
                            if x >= win.x as f64
                                && x <= (win.x + win.width) as f64
                                && y >= win.y as f64
                                && y <= (win.y + win.height) as f64
                            {
                                next_hovered_window = Some(i);
                                break;
                            }
                        }
                    }

                    let (
                        next_hover_tool_index,
                        next_hover_size_panel,
                        next_hover_crop_panel,
                        next_hover_record_tile,
                        cursor_name,
                    ) = match hit {
                        Some(ToolbarHit::Tool(index)) if !st.recording.panel_open => {
                            (Some(index), false, false, None, "pointer")
                        }
                        Some(ToolbarHit::SizePanel) if !st.recording.panel_open => {
                            (None, true, false, None, "default")
                        }
                        Some(ToolbarHit::CropPanel) if !st.recording.panel_open => {
                            (None, false, true, None, "pointer")
                        }
                        None => {
                            if let Some(tile) = record_hit {
                                (None, false, false, Some(tile), "pointer")
                            } else {
                                let c = if st.completed || st.is_dragging {
                                    if st.recording.panel_open
                                        && st.recording.rec_webcam
                                        && webcam_preview_rect(&st, rect).contains(x, y)
                                    {
                                        "fleur"
                                    } else {
                                        detect_resize_handle(x, y, rect)
                                            .map(cursor_name_for_handle)
                                            .unwrap_or_else(|| {
                                                if is_inside_selection(x, y, rect) {
                                                    "fleur"
                                                } else {
                                                    "crosshair"
                                                }
                                            })
                                    }
                                } else if next_hovered_window.is_some() {
                                    "pointer"
                                } else {
                                    "crosshair"
                                };
                                (None, false, false, None, c)
                            }
                        }
                        _ => (None, false, false, None, "crosshair"),
                    };

                    let hover_changed = st.hover_tool_index != next_hover_tool_index
                        || st.hover_size_panel != next_hover_size_panel
                        || st.hover_crop_panel != next_hover_crop_panel
                        || st.recording.hover_record_tile != next_hover_record_tile
                        || st.hovered_window != next_hovered_window;

                    st.hover_tool_index = next_hover_tool_index;
                    st.hover_size_panel = next_hover_size_panel;
                    st.hover_crop_panel = next_hover_crop_panel;
                    st.recording.hover_record_tile = next_hover_record_tile;
                    st.hovered_window = next_hovered_window;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;

                    (cursor_name.to_string(), hover_changed, false)
                }
            }
        };

        if let Some(win) = window_weak_motion.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name(&cursor_name, None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if hover_changed {
            if let Some(drawing_area) = drawing_area_weak_motion.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    let state_motion_leave = state.clone();
    let drawing_area_weak_leave = drawing_area.downgrade();
    let window_weak_leave = window.downgrade();
    motion_controller.connect_leave(move |_| {
        let mut st = state_motion_leave.lock().unwrap();
        let was_hovering = st.hover_tool_index.is_some()
            || st.hover_size_panel
            || st.hover_crop_panel
            || st.recording.hover_record_tile.is_some()
            || st.hovered_capture_crop_menu_item != -1
            || st.recording.hovered_crop_menu_item != -1
            || st.recording.hovered_settings_item != -1;
        st.hover_tool_index = None;
        st.hover_size_panel = false;
        st.hover_crop_panel = false;
        st.recording.hover_record_tile = None;
        st.hovered_capture_crop_menu_item = -1;
        st.recording.hovered_crop_menu_item = -1;
        st.recording.hovered_settings_item = -1;
        drop(st);

        // Reset cursor
        if let Some(win) = window_weak_leave.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name("crosshair", None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if was_hovering {
            if let Some(drawing_area) = drawing_area_weak_leave.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    drawing_area.add_controller(motion_controller);

    // Toolbar click actions
    let click_gesture = GestureClick::builder()
        .button(1)
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_click = state.clone();
    let drawing_area_weak_click = drawing_area.downgrade();
    let result_tx_click = result_tx.clone();
    let window_weak_click = window.downgrade();
    let background_click = background.clone();
    click_gesture.connect_pressed(move |_, n_press, x, y| {
        let mut st = state_click.lock().unwrap();
        let rect = current_selection_rect(&st);
        let recording_panel_open = st.recording.panel_open;

        // ── Menu click handling ──

        // Capture crop menu (non-recording mode)
        if st.capture_crop_menu_open {
            if let Some(item) = capture_crop_menu_hit_item(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                st.capture_aspect_ratio_index = item;
                apply_aspect_to_selection(
                    &mut st,
                    aspect_ratio_for_index(item),
                    screen_width as f64,
                    screen_height as f64,
                );
                st.capture_crop_menu_open = false;
                st.hovered_capture_crop_menu_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
            if capture_crop_menu_contains(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                // Click was inside the crop menu (empty area) — ignore it
                return;
            }
            st.capture_crop_menu_open = false;
            st.hovered_capture_crop_menu_item = -1;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() {
                da.queue_draw();
            }
            return;
        }

        // Recording crop menu
        if st.recording.crop_menu_open {
            if let Some(item) = recording_crop_menu_hit_item(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                st.recording.record_aspect_ratio_index = item;
                apply_aspect_to_selection(
                    &mut st,
                    aspect_ratio_for_index(item),
                    screen_width as f64,
                    screen_height as f64,
                );
                st.recording.crop_menu_open = false;
                st.recording.hovered_crop_menu_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
            if recording_crop_menu_contains(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                // Click was inside the recording crop menu (empty area) — ignore it
                return;
            }
            st.recording.crop_menu_open = false;
            st.recording.hovered_crop_menu_item = -1;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() {
                da.queue_draw();
            }
            return;
        }

        // Settings menu
        if st.recording.settings_menu_open {
            // If a dropdown is open, check dropdown item clicks first
            if let Some(drop_idx) = st.recording.settings_dropdown_open {
                let tab = match st.recording.settings_tab {
                    SettingsTab::Video => 1,
                    SettingsTab::Gif => 2,
                    _ => 0,
                };
                let (options, value_ptr): (&[&str], &mut usize) = if tab == 1 && drop_idx == 3 {
                    (
                        &["Original", "1080p", "720p"],
                        &mut st.recording.video_max_res,
                    )
                } else if tab == 1 && drop_idx == 4 {
                    (&["24", "30", "50", "60"], &mut st.recording.video_fps)
                } else if tab == 2 && drop_idx == 6 {
                    (
                        &["800 x auto", "640 x auto", "480 x auto", "Original"],
                        &mut st.recording.gif_size_idx,
                    )
                } else {
                    (&[], &mut 0)
                };
                // Compute dropdown popup rect
                let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                    .clamp(10.0, screen_width as f64 - 450.0);
                let menu_y = (rect.top + 24.0).clamp(10.0, screen_height as f64 - 570.0);
                let popup_y = compute_dropdown_popup_y(
                    menu_y,
                    drop_idx,
                    match tab {
                        1 => SettingsTab::Video,
                        2 => SettingsTab::Gif,
                        _ => SettingsTab::General,
                    },
                );
                let popup_rect = RectF {
                    x: menu_x + 130.0,
                    y: popup_y,
                    width: 140.0,
                    height: options.len() as f64 * 30.0,
                };
                // Check if clicked outside popup
                if !popup_rect.contains(x, y) {
                    st.recording.settings_dropdown_open = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_click.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }
                // Check item clicks
                for (oi, _opt) in options.iter().enumerate() {
                    let item_rect = RectF {
                        x: popup_rect.x,
                        y: popup_rect.y + oi as f64 * 30.0,
                        width: popup_rect.width,
                        height: 30.0,
                    };
                    if item_rect.contains(x, y) {
                        *value_ptr = oi;
                        st.recording.settings_dropdown_open = None;
                        st.recording.hovered_settings_item = -1;
                        drop(st);
                        if let Some(da) = drawing_area_weak_click.upgrade() {
                            da.queue_draw();
                        }
                        return;
                    }
                }
                st.recording.settings_dropdown_open = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }

            if let Some(item) = settings_menu_hit_item(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
                st.recording.settings_tab,
            ) {
                // Tab clicks
                if item < 3 {
                    st.recording.settings_tab = match item {
                        0 => SettingsTab::General,
                        1 => SettingsTab::Video,
                        _ => SettingsTab::Gif,
                    };
                    st.recording.hovered_settings_item = -1;
                    st.recording.settings_dropdown_open = None;
                } else if matches!(st.recording.settings_tab, SettingsTab::General) {
                    let general_idx = item - 3;
                    match general_idx {
                        0 => st.recording.rec_controls = !st.recording.rec_controls,
                        1 => st.recording.display_rec_time = !st.recording.display_rec_time,
                        2 => st.recording.hidpi = !st.recording.hidpi,
                        3 => st.recording.do_not_disturb = !st.recording.do_not_disturb,
                        4 => st.recording.show_cursor = !st.recording.show_cursor,
                        5 => st.recording.rec_clicks = !st.recording.rec_clicks,
                        6 => st.recording.rec_keystrokes = !st.recording.rec_keystrokes,
                        7 => st.recording.remember_selection = !st.recording.remember_selection,
                        8 => st.recording.dim_screen = !st.recording.dim_screen,
                        9 => st.recording.show_countdown = !st.recording.show_countdown,
                        _ => {}
                    }
                    if !st.recording.rec_clicks {
                        st.recording.click_options_open = false;
                        st.recording.click_dropdown_open = None;
                        st.recording.click_previews.clear();
                    }
                    st.recording.settings_dropdown_open = None;
                } else if matches!(st.recording.settings_tab, SettingsTab::Video) {
                    let video_idx = item - 3;
                    match video_idx {
                        0 => st.recording.settings_dropdown_open = Some(3), // res dropdown
                        1 => st.recording.settings_dropdown_open = Some(4), // fps dropdown
                        2 => st.recording.record_mono = !st.recording.record_mono,
                        3 => st.recording.open_editor = !st.recording.open_editor,
                        _ => {}
                    }
                } else if matches!(st.recording.settings_tab, SettingsTab::Gif) {
                    let gif_idx = item - 3;
                    let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                        .clamp(10.0, screen_width as f64 - 450.0);
                    let value_x = menu_x + 130.0;
                    match gif_idx {
                        0 => {
                            // FPS slider — click-to-position + start drag
                            let slider_x = value_x + 55.0;
                            let slider_w = 220.0;
                            let click_x = x.clamp(slider_x, slider_x + slider_w);
                            st.recording.gif_fps = 5.0 + (click_x - slider_x) / slider_w * 55.0;
                            st.recording.gif_slider_dragging = Some(0);
                        }
                        1 => {
                            // Quality slider — click-to-position + start drag
                            let q_slider_w = 160.0;
                            let click_x = x.clamp(value_x, value_x + q_slider_w);
                            st.recording.gif_quality =
                                ((click_x - value_x) / q_slider_w).clamp(0.0, 1.0);
                            st.recording.gif_slider_dragging = Some(1);
                        }
                        2 => st.recording.optimize_gif = !st.recording.optimize_gif,
                        3 => st.recording.settings_dropdown_open = Some(6), // size dropdown
                        _ => {}
                    }
                }
                st.recording.hovered_settings_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
            if settings_menu_contains(
                rect.left,
                rect.top,
                rect.width(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                // Click was inside the settings menu (empty area) — ignore it
                return;
            }
            // Click outside settings menu closes it
            st.recording.settings_menu_open = false;
            st.recording.hovered_settings_item = -1;
            st.recording.settings_dropdown_open = None;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() {
                da.queue_draw();
            }
            return;
        }

        // ── Click options menu click handling ──
        if st.recording.click_options_open {
            if let Some(dropdown) = st.recording.click_dropdown_open {
                if let Some(item) = click_dropdown_hit_item(
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                    screen_width as f64,
                    screen_height as f64,
                    x,
                    y,
                    dropdown,
                ) {
                    match dropdown {
                        1 => st.recording.click_color = item.min(CLICK_COLORS_LEN - 1),
                        2 => st.recording.click_style = item.min(1),
                        _ => {}
                    }
                    st.recording.click_dropdown_open = None;
                    st.recording.hovered_click_item = -1;
                    drop(st);
                    if let Some(da) = drawing_area_weak_click.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }
                st.recording.click_dropdown_open = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }

            if let Some(item) = click_options_hit_item(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                match item {
                    0 => {
                        // Size slider — start drag
                        let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                            .clamp(10.0, screen_width as f64 - 450.0);
                        let value_x = menu_x + 130.0;
                        let slider_x = value_x;
                        let slider_w = 280.0;
                        let click_x = x.clamp(slider_x, slider_x + slider_w);
                        st.recording.click_size = ((click_x - slider_x) / slider_w).clamp(0.0, 1.0);
                        st.recording.click_slider_dragging = true;
                    }
                    1 => {
                        st.recording.click_dropdown_open = Some(1);
                    }
                    2 => {
                        st.recording.click_dropdown_open = Some(2);
                    }
                    3 => {
                        // Animation toggle
                        st.recording.click_animate = !st.recording.click_animate;
                    }
                    4 => {
                        let was_empty = st.recording.click_previews.is_empty();
                        st.recording
                            .click_previews
                            .push((x, y, std::time::Instant::now()));
                        if st.recording.click_previews.len() > 10 {
                            st.recording.click_previews.remove(0);
                        }
                        if was_empty {
                            let state_timer = state_click.clone();
                            let drawing_area_timer = drawing_area_weak_click.clone();
                            glib::timeout_add_local(
                                std::time::Duration::from_millis(16),
                                move || {
                                    let mut st = state_timer.lock().unwrap();
                                    let click_lifetime = std::time::Duration::from_millis(1500);
                                    st.recording.click_previews.retain(|&(_, _, birth_time)| {
                                        birth_time.elapsed() < click_lifetime
                                    });
                                    if let Some(da) = drawing_area_timer.upgrade() {
                                        da.queue_draw();
                                    }
                                    if st.recording.click_previews.is_empty() {
                                        glib::ControlFlow::Break
                                    } else {
                                        glib::ControlFlow::Continue
                                    }
                                },
                            );
                        }
                    }
                    _ => {
                        // Done button — close
                        st.recording.click_options_open = false;
                        st.recording.hovered_click_item = -1;
                        st.recording.click_dropdown_open = None;
                        st.recording.click_previews.clear();
                    }
                }
                st.recording.hovered_click_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
            if click_options_menu_contains(
                rect.left,
                rect.top,
                rect.width(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                // Click was inside click options (empty area) — ignore it
                return;
            }
            // Click outside click options closes it
            st.recording.click_options_open = false;
            st.recording.hovered_click_item = -1;
            st.recording.click_slider_dragging = false;
            st.recording.click_dropdown_open = None;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() {
                da.queue_draw();
            }
            return;
        }

        // ── Scroll popup click handling ──
        if st.scroll_popup_open {
            let (cx, cy) = if st.completed || st.is_dragging {
                let r = current_selection_rect(&st);
                (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
            } else {
                (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
            };
            let popup_w = 360.0;
            let popup_h = 170.0;
            let popup_x = cx - popup_w / 2.0;
            let popup_y = cy - popup_h / 2.0;
            let close_size = 22.0;
            let close_x = popup_x + popup_w - close_size - 10.0;
            let close_y = popup_y + 10.0;

            // Check if close button clicked
            if x >= close_x
                && x <= close_x + close_size
                && y >= close_y
                && y <= close_y + close_size
            {
                st.scroll_popup_open = false;
                st.hovered_scroll_popup_close = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }

            // Click outside popup closes it
            if !(x >= popup_x && x <= popup_x + popup_w && y >= popup_y && y <= popup_y + popup_h) {
                st.scroll_popup_open = false;
                st.hovered_scroll_popup_close = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
        }

        // ── Window picker popup click handling ──
        if st.window_picker_open {
            const POPUP_W: f64 = 320.0;
            const ITEM_H: f64 = 28.0;
            const HEADER_H: f64 = 30.0;
            const PAD: f64 = 8.0;
            let n = st.windows.len();
            let popup_h = PAD * 2.0 + HEADER_H + n as f64 * ITEM_H;
            let (center_x, center_y) = if st.completed || st.is_dragging {
                let r = current_selection_rect(&st);
                (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
            } else {
                (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
            };
            let popup_x = (center_x - POPUP_W / 2.0)
                .clamp(10.0, (screen_width as f64 - POPUP_W - 10.0).max(10.0));
            let popup_y = (center_y - popup_h / 2.0)
                .clamp(10.0, (screen_height as f64 - popup_h - 10.0).max(10.0));
            let list_y = popup_y + PAD + HEADER_H;

            // Click outside popup closes it
            if !(x >= popup_x && x <= popup_x + POPUP_W && y >= popup_y && y <= popup_y + popup_h) {
                st.window_picker_open = false;
                st.hovered_window_picker_entry = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }

            // Check if a window entry was clicked
            let entry_index = ((y - list_y) / ITEM_H) as i32;
            if entry_index >= 0 && (entry_index as usize) < st.windows.len() {
                let win = st.windows[entry_index as usize].clone();
                let w = win.width as f64;
                let h = win.height as f64;
                st.start_x = win.x as f64;
                st.start_y = win.y as f64;
                st.current_x = win.x as f64 + w;
                st.current_y = win.y as f64 + h;
                st.completed = true;
                st.window_picker_open = false;
                st.hovered_window_picker_entry = -1;
                drop(st);
                if let Some(window) = window_weak_click.upgrade() {
                    send_selection_result(
                        &state_click,
                        &result_tx_click,
                        &window,
                        screen_width,
                        screen_height,
                        background_click.as_ref(),
                    );
                }
                return;
            }

            // Click on header area inside popup — ignore
            return;
        }

        // ── Webcam options menu click handling ──
        if st.recording.webcam_options_open {
            if let Some(item) = webcam_options_hit_item(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                match item {
                    0 => {
                        st.recording.webcam_device = -1;
                        st.recording.webcam_preview = None;
                        st.recording.webcam_frame = None;
                    }
                    1 => st.recording.webcam_size = 0,
                    2 => st.recording.webcam_size = 1,
                    3 => st.recording.webcam_size = 2,
                    4 => st.recording.webcam_size = 3,
                    5 => st.recording.webcam_size = 4,
                    6 => st.recording.webcam_shape = 0,
                    7 => st.recording.webcam_shape = 1,
                    8 => st.recording.webcam_shape = 2,
                    9 => st.recording.webcam_shape = 3,
                    10 => {
                        st.recording.webcam_flip = !st.recording.webcam_flip;
                        st.recording.webcam_preview = None;
                        st.recording.webcam_frame = None;
                    }
                    device_id if device_id >= 100 => {
                        st.recording.webcam_device = device_id - 100;
                        st.recording.rec_webcam = true;
                        st.recording.webcam_preview = None;
                        st.recording.webcam_frame = None;
                    }
                    _ => {}
                }
                sync_webcam_preview(&mut st);
                st.recording.hovered_webcam_item = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
                return;
            }
            if webcam_options_menu_contains(
                rect.left,
                rect.top,
                rect.width(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                // Click was inside webcam options (empty area) — ignore it
                return;
            }
            st.recording.webcam_options_open = false;
            st.recording.hovered_webcam_item = -1;
            drop(st);
            if let Some(da) = drawing_area_weak_click.upgrade() {
                da.queue_draw();
            }
            return;
        }

        // ── Normal click handling (no menus open) ──
        let record_hit = if recording_panel_open {
            recording_tile_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            )
        } else {
            None
        };
        let hit = if recording_panel_open {
            None
        } else {
            toolbar_hit_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            )
        };
        let clicked = match hit {
            Some(ToolbarHit::Tool(index)) if !recording_panel_open => Some(TOOLBAR_ICONS[index]),
            _ => None,
        };

        match clicked {
            Some(ToolbarIcon::Fullscreen) => {
                st.active_tool_index = TOOLBAR_FULLSCREEN_INDEX;
                st.intent = OverlayIntent::Area;
                st.start_x = 0.0;
                st.start_y = 0.0;
                st.current_x = screen_width as f64;
                st.current_y = screen_height as f64;
                st.completed = true;
                st.is_dragging = false;
                st.fullscreen_mode = true;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Area) => {
                st.active_tool_index = TOOLBAR_AREA_INDEX;
                st.intent = OverlayIntent::Area;
                let screen_w = screen_width as f64;
                let screen_h = screen_height as f64;
                let sel_w = DEFAULT_SELECTION_WIDTH
                    .min(screen_w)
                    .max(MIN_SELECTION_WIDTH.min(screen_w));
                let sel_h = DEFAULT_SELECTION_HEIGHT
                    .min(screen_h)
                    .max(MIN_SELECTION_HEIGHT.min(screen_h));
                let sel_x = ((screen_w - sel_w) / 2.0).max(0.0);
                let sel_y = ((screen_h - sel_h) / 2.0).max(0.0);
                st.start_x = sel_x;
                st.start_y = sel_y;
                st.current_x = sel_x + sel_w;
                st.current_y = sel_y + sel_h;
                st.completed = true;
                st.is_dragging = false;
                st.fullscreen_mode = false;
                st.recording.panel_open = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Recording) => {
                st.active_tool_index = TOOLBAR_RECORDING_INDEX;
                st.recording.panel_open = true;
                st.intent = OverlayIntent::Record;
                st.hover_tool_index = None;
                st.hover_size_panel = false;
                st.hover_crop_panel = false;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Timer) => {
                if !st.timer_delay_active {
                    st.timer_delay_active = true;
                    if st.capture_delay_seconds <= 0 {
                        st.capture_delay_seconds = 5;
                    }
                } else {
                    st.capture_delay_seconds = match st.capture_delay_seconds {
                        3 => 5,
                        5 => 10,
                        _ => 0,
                    };
                    st.timer_delay_active = st.capture_delay_seconds > 0;
                }
                st.timer_delay_active = st.capture_delay_seconds > 0;
                st.hover_tool_index = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Scroll) => {
                st.capture_crop_menu_open = false;
                st.scroll_popup_open = true;
                st.active_tool_index = TOOLBAR_SCROLL_INDEX;
                st.intent = OverlayIntent::Area;
                st.hover_tool_index = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Window) => {
                st.capture_crop_menu_open = false;
                st.scroll_popup_open = false;
                st.active_tool_index = TOOLBAR_WINDOW_INDEX;
                st.intent = OverlayIntent::Area;
                st.hover_tool_index = None;
                st.window_picker_open = true;
                st.hovered_window_picker_entry = -1;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            Some(ToolbarIcon::Ocr) => {
                st.active_tool_index = 5;
                st.intent = OverlayIntent::Ocr;
                st.hover_tool_index = None;
                drop(st);
                if let Some(da) = drawing_area_weak_click.upgrade() {
                    da.queue_draw();
                }
            }
            _ => {
                // Crop card clicked — open toolbar crop menu
                if !recording_panel_open && hit == Some(ToolbarHit::CropPanel) {
                    st.capture_crop_menu_open = !st.capture_crop_menu_open;
                    st.hovered_capture_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_click.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }

                // Recording panel tile clicks
                if recording_panel_open {
                    if let Some(tile) = record_hit {
                        match tile {
                            RecordPanelTile::Crop => {
                                st.recording.crop_menu_open = !st.recording.crop_menu_open;
                                st.recording.hovered_crop_menu_item = -1;
                                st.recording.settings_menu_open = false;
                                st.recording.settings_dropdown_open = None;
                                st.recording.click_options_open = false;
                                st.recording.click_dropdown_open = None;
                                st.recording.webcam_options_open = false;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Controls => {
                                st.recording.settings_menu_open = !st.recording.settings_menu_open;
                                st.recording.hovered_settings_item = -1;
                                st.recording.settings_dropdown_open = None;
                                st.recording.crop_menu_open = false;
                                st.recording.click_options_open = false;
                                st.recording.click_dropdown_open = None;
                                st.recording.webcam_options_open = false;
                                st.recording.hover_record_tile = None;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Mic => {
                                st.recording.mic_toggle = !st.recording.mic_toggle
                            }
                            RecordPanelTile::Speaker => {
                                st.recording.speaker_toggle = !st.recording.speaker_toggle
                            }
                            RecordPanelTile::Clicks => {
                                if st.recording.rec_clicks {
                                    st.recording.rec_clicks = false;
                                    st.recording.click_options_open = false;
                                    st.recording.click_previews.clear();
                                } else {
                                    st.recording.rec_clicks = true;
                                    st.recording.click_options_open = true;
                                }
                                st.recording.crop_menu_open = false;
                                st.recording.settings_menu_open = false;
                                st.recording.settings_dropdown_open = None;
                                st.recording.webcam_options_open = false;
                                st.recording.click_dropdown_open = None;
                                st.recording.hovered_click_item = -1;
                                st.recording.click_slider_dragging = false;
                                st.recording.hover_record_tile = None;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Webcam => {
                                st.recording.rec_webcam = !st.recording.rec_webcam;
                                if st.recording.rec_webcam && st.recording.webcam_device < 0 {
                                    if let Some(device) = first_webcam_device() {
                                        st.recording.webcam_device = device;
                                    }
                                }
                                sync_webcam_preview(&mut st);
                                st.recording.crop_menu_open = false;
                                st.recording.settings_menu_open = false;
                                st.recording.settings_dropdown_open = None;
                                st.recording.click_options_open = false;
                                st.recording.click_dropdown_open = None;
                                st.recording.webcam_options_open = false;
                                st.recording.hovered_webcam_item = -1;
                                st.recording.hover_record_tile = None;
                                st.hover_tool_index = None;
                            }
                            RecordPanelTile::Keystrokes => {
                                st.recording.rec_keystrokes = !st.recording.rec_keystrokes
                            }
                            RecordPanelTile::Size => {}
                            RecordPanelTile::RecordVideo | RecordPanelTile::RecordGif => {
                                let record_type = if matches!(tile, RecordPanelTile::RecordGif) {
                                    RecordingType::Gif
                                } else {
                                    RecordingType::Video
                                };
                                let request = recording_request_from_state(&st, record_type);
                                drop(st);
                                let _ =
                                    result_tx_click.send(Ok(OverlaySelection::Recording(request)));
                                if let Some(window) = window_weak_click.upgrade() {
                                    window.close();
                                }
                                return;
                            }
                        }
                        drop(st);
                        if let Some(da) = drawing_area_weak_click.upgrade() {
                            da.queue_draw();
                        }
                        return;
                    }
                }

                if n_press == 2 {
                    drop(st);
                    let st = state_click.lock().unwrap();
                    let inside_selection =
                        st.completed && is_inside_selection(x, y, current_selection_rect(&st));
                    drop(st);

                    if inside_selection {
                        if let Some(window) = window_weak_click.upgrade() {
                            send_selection_result(
                                &state_click,
                                &result_tx_click,
                                &window,
                                screen_width,
                                screen_height,
                                background_click.as_ref(),
                            );
                        }
                    }
                } else {
                    drop(st);
                }
            }
        }
    });
    drawing_area.add_controller(click_gesture);

    // Right-click gesture for recording panel tile menus
    let right_click_gesture = GestureClick::builder()
        .button(3)
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();
    let state_rc = state.clone();
    let drawing_area_weak_rc = drawing_area.downgrade();
    right_click_gesture.connect_pressed(move |_, _n_press, x, y| {
        let mut st = state_rc.lock().unwrap();
        let rect = current_selection_rect(&st);
        let recording_panel_open = st.recording.panel_open;
        if recording_panel_open {
            if let Some(tile) = recording_tile_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                x,
                y,
            ) {
                match tile {
                    RecordPanelTile::Clicks => {
                        st.recording.click_options_open = !st.recording.click_options_open;
                        st.recording.click_dropdown_open = None;
                        st.recording.hovered_click_item = -1;
                        st.recording.click_slider_dragging = false;
                        st.recording.settings_menu_open = false;
                        st.recording.crop_menu_open = false;
                        st.recording.webcam_options_open = false;
                        st.recording.hover_record_tile = None;
                        st.hover_tool_index = None;
                    }
                    RecordPanelTile::Webcam => {
                        st.recording.webcam_options_open = !st.recording.webcam_options_open;
                        st.recording.hovered_webcam_item = -1;
                        st.recording.settings_menu_open = false;
                        st.recording.crop_menu_open = false;
                        st.recording.click_options_open = false;
                        st.recording.click_dropdown_open = None;
                        st.recording.hover_record_tile = None;
                        st.hover_tool_index = None;
                    }
                    _ => {}
                }
                drop(st);
                if let Some(da) = drawing_area_weak_rc.upgrade() {
                    da.queue_draw();
                }
                return;
            }
        }
        drop(st);
    });
    drawing_area.add_controller(right_click_gesture);

    // Setup drag gesture for area selection
    let drag_gesture = GestureDrag::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_drag = state.clone();
    let drawing_area_weak = drawing_area.downgrade();
    let result_tx_drag = result_tx.clone();
    let window_weak_drag = window.downgrade();
    let background_drag = background.clone();

    // Note: connect_drag_begin takes 3 params (gesture, x, y)
    drag_gesture.connect_drag_begin(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            let (start_x, start_y) =
                clamp_point_to_bounds(x, y, screen_width as f64, screen_height as f64);

            if st.overlay_mode == OverlayMode::CrosshairCapture {
                st.drag_origin_x = start_x;
                st.drag_origin_y = start_y;
                st.start_x = start_x;
                st.start_y = start_y;
                st.current_x = start_x;
                st.current_y = start_y;
                st.drag_mode = Some(DragMode::NewSelection);
                st.initial_rect = None;
                st.is_dragging = true;
                st.completed = false;
                st.active_tool_index = TOOLBAR_AREA_INDEX;
                drop(st);

                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }

            let rect = current_selection_rect(&st);

            // Suppress drag when clicking toolbar tools, size/crop panels
            if toolbar_item_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                start_x,
                start_y,
            )
            .is_some()
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            let hit = toolbar_hit_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                start_x,
                start_y,
            );
            if matches!(
                hit,
                Some(ToolbarHit::CropPanel) | Some(ToolbarHit::SizePanel)
            ) {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Suppress drag when clicking recording panel tiles
            if st.recording.panel_open
                && recording_tile_at(
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                    screen_width as f64,
                    screen_height as f64,
                    start_x,
                    start_y,
                )
                .is_some()
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Any open menu owns this pointer press. The click handler may
            // close the menu or update a slider, but area move/resize/new
            // selection must not also start underneath it.
            if st.capture_crop_menu_open
                || st.recording.crop_menu_open
                || st.recording.settings_menu_open
                || st.recording.click_options_open
                || st.recording.webcam_options_open
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                st.recording.dragging_webcam = false;
                drop(st);
                return;
            }

            if st.recording.panel_open && st.recording.rec_webcam {
                let preview = webcam_preview_rect(&st, rect);
                if preview.contains(start_x, start_y) {
                    st.drag_origin_x = start_x;
                    st.drag_origin_y = start_y;
                    st.recording.dragging_webcam = true;
                    st.recording.webcam_drag_offset_x = start_x - preview.x;
                    st.recording.webcam_drag_offset_y = start_y - preview.y;
                    st.is_dragging = false;
                    st.drag_mode = None;
                    st.initial_rect = None;
                    drop(st);
                    return;
                }
            }

            st.drag_origin_x = start_x;
            st.drag_origin_y = start_y;
            st.initial_rect = Some(current_selection_rect(&st));

            let drag_mode = if st.completed {
                let rect = current_selection_rect(&st);
                if let Some(handle) = detect_resize_handle(start_x, start_y, rect) {
                    // Cursor is on a border/corner handle — resize.
                    DragMode::Resize(handle)
                } else if is_inside_selection(start_x, start_y, rect) {
                    // Cursor is inside the selection — move the whole rect.
                    DragMode::Move
                } else {
                    // Cursor is outside the selection — start a new one.
                    DragMode::NewSelection
                }
            } else {
                DragMode::NewSelection
            };

            st.drag_mode = Some(drag_mode);

            if matches!(drag_mode, DragMode::NewSelection) {
                if let Some(win_idx) = st.hovered_window {
                    let win = &st.windows[win_idx];
                    let (wx, wy, ww, wh) = (
                        win.x as f64,
                        win.y as f64,
                        win.width as f64,
                        win.height as f64,
                    );
                    st.start_x = wx;
                    st.start_y = wy;
                    st.current_x = wx + ww;
                    st.current_y = wy + wh;
                    st.completed = true;
                    st.is_dragging = false;
                    st.drag_mode = None;
                } else {
                    st.start_x = start_x;
                    st.start_y = start_y;
                    st.current_x = start_x;
                    st.current_y = start_y;
                    st.completed = false;
                    st.is_dragging = true;
                }
                st.fullscreen_mode = false;
                if !st.recording.panel_open {
                    st.active_tool_index = TOOLBAR_AREA_INDEX;
                }
            } else {
                st.is_dragging = true;
            }
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_update(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.recording.dragging_webcam {
                let pointer_x = st.drag_origin_x + x;
                let pointer_y = st.drag_origin_y + y;
                let rect = current_selection_rect(&st);
                let top_left_x = pointer_x - st.recording.webcam_drag_offset_x;
                let top_left_y = pointer_y - st.recording.webcam_drag_offset_y;
                set_webcam_preview_top_left(&mut st, rect, top_left_x, top_left_y);
                drop(st);
                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }
            if st.recording.gif_slider_dragging.is_some() || st.recording.click_slider_dragging {
                drop(st);
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            let ratio = active_aspect_ratio(&st);
            if ratio > 0.0 && !matches!(st.drag_mode, Some(DragMode::Move)) {
                apply_aspect_to_selection(
                    &mut st,
                    ratio,
                    screen_width as f64,
                    screen_height as f64,
                );
            }
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_end(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        #[strong]
        result_tx_drag,
        #[strong]
        window_weak_drag,
        #[strong]
        background_drag,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.recording.dragging_webcam {
                let pointer_x = st.drag_origin_x + x;
                let pointer_y = st.drag_origin_y + y;
                let rect = current_selection_rect(&st);
                let top_left_x = pointer_x - st.recording.webcam_drag_offset_x;
                let top_left_y = pointer_y - st.recording.webcam_drag_offset_y;
                set_webcam_preview_top_left(&mut st, rect, top_left_x, top_left_y);
                st.recording.dragging_webcam = false;
                st.recording.webcam_drag_offset_x = 0.0;
                st.recording.webcam_drag_offset_y = 0.0;
                drop(st);
                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }
            if st.recording.gif_slider_dragging.is_some() || st.recording.click_slider_dragging {
                st.recording.gif_slider_dragging = None;
                st.recording.click_slider_dragging = false;
                drop(st);
                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            let ratio = active_aspect_ratio(&st);
            if ratio > 0.0 && !matches!(st.drag_mode, Some(DragMode::Move)) {
                apply_aspect_to_selection(
                    &mut st,
                    ratio,
                    screen_width as f64,
                    screen_height as f64,
                );
            }
            st.is_dragging = false;
            st.completed = true;
            st.drag_mode = None;
            st.initial_rect = None;
            let is_crosshair = st.overlay_mode == OverlayMode::CrosshairCapture;
            drop(st);

            if is_crosshair {
                if let Some(window) = window_weak_drag.upgrade() {
                    send_selection_result(
                        &state_drag,
                        &result_tx_drag,
                        &window,
                        screen_width,
                        screen_height,
                        background_drag.as_ref(),
                    );
                }
                return;
            }

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drawing_area.add_controller(drag_gesture);

    // Setup keyboard controller for ESC key
    let key_controller = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_key = state.clone();
    let window_weak_esc = window.downgrade();
    let result_tx_esc = result_tx.clone();
    let background_key = background.clone();
    let drawing_area_weak_key = drawing_area.downgrade();

    key_controller.connect_key_pressed(clone!(
        #[strong]
        state_key,
        move |_, key, _, _| {
            if key == Key::Escape {
                let mut st = state_key.lock().unwrap();
                st.cancelled = true;
                st.fullscreen_mode = false;
                drop(st);

                let _ = result_tx_esc.send(Ok(OverlaySelection::Area(None)));

                if let Some(window) = window_weak_esc.upgrade() {
                    window.close();
                }

                return glib::Propagation::Stop;
            }

            if key == Key::Return
                || key == Key::KP_Enter
                || key == Key::ISO_Enter
                || key == Key::space
            {
                let mut st = state_key.lock().unwrap();
                if st.timer_delay_active && st.capture_delay_seconds > 0 && !st.countdown_active {
                    st.countdown_active = true;
                    st.countdown_cancel_requested = false;
                    st.countdown_value = st.capture_delay_seconds;
                    st.hovered_countdown_cancel = false;
                    drop(st);

                    if let Some(da) = drawing_area_weak_key.upgrade() {
                        da.queue_draw();
                    }

                    let state_countdown = state_key.clone();
                    let result_tx_countdown = result_tx_esc.clone();
                    let window_weak_countdown = window_weak_esc.clone();
                    let drawing_area_weak_countdown = drawing_area_weak_key.clone();
                    let background_countdown = background_key.clone();
                    glib::timeout_add_seconds_local(1, move || {
                        let mut st = state_countdown.lock().unwrap();
                        if st.countdown_cancel_requested || st.cancelled {
                            st.countdown_active = false;
                            st.countdown_cancel_requested = false;
                            drop(st);
                            if let Some(da) = drawing_area_weak_countdown.upgrade() {
                                da.queue_draw();
                            }
                            return glib::ControlFlow::Break;
                        }

                        st.countdown_value -= 1;
                        if st.countdown_value <= 0 {
                            st.countdown_active = false;
                            drop(st);
                            if let Some(window) = window_weak_countdown.upgrade() {
                                send_selection_result(
                                    &state_countdown,
                                    &result_tx_countdown,
                                    &window,
                                    screen_width,
                                    screen_height,
                                    background_countdown.as_ref(),
                                );
                            }
                            glib::ControlFlow::Break
                        } else {
                            drop(st);
                            if let Some(da) = drawing_area_weak_countdown.upgrade() {
                                da.queue_draw();
                            }
                            glib::ControlFlow::Continue
                        }
                    });
                } else if !st.countdown_active {
                    drop(st);
                    if let Some(window) = window_weak_esc.upgrade() {
                        send_selection_result(
                            &state_key,
                            &result_tx_esc,
                            &window,
                            screen_width,
                            screen_height,
                            background_key.as_ref(),
                        );
                    }
                }

                return glib::Propagation::Stop;
            }

            let delta = match key {
                Key::Left => Some((-1.0, 0.0)),
                Key::Right => Some((1.0, 0.0)),
                Key::Up => Some((0.0, -1.0)),
                Key::Down => Some((0.0, 1.0)),
                _ => None,
            };

            if let Some((dx, dy)) = delta {
                let mut st = state_key.lock().unwrap();
                if st.completed {
                    let rect = current_selection_rect(&st);
                    let next = SelectionRectF {
                        left: (rect.left + dx)
                            .clamp(0.0, (screen_width as f64 - rect.width()).max(0.0)),
                        top: (rect.top + dy)
                            .clamp(0.0, (screen_height as f64 - rect.height()).max(0.0)),
                        right: 0.0,
                        bottom: 0.0,
                    };
                    let moved = SelectionRectF {
                        right: next.left + rect.width(),
                        bottom: next.top + rect.height(),
                        ..next
                    };
                    set_selection_rect(&mut st, moved);
                    st.fullscreen_mode = false;
                    st.active_tool_index = TOOLBAR_AREA_INDEX;
                    drop(st);
                    if let Some(drawing_area) = drawing_area_weak_key.upgrade() {
                        drawing_area.queue_draw();
                    }
                    return glib::Propagation::Stop;
                }
            }

            glib::Propagation::Proceed
        }
    ));

    window.add_controller(key_controller);

    // On X11: set compositor-bypass hints as soon as the native window is
    // realized (XID assigned) but BEFORE it is mapped/shown.  Using
    // connect_realize instead of connect_map means the compositor sees the
    // correct _NET_WM_WINDOW_TYPE and _NET_WM_BYPASS_COMPOSITOR on the very
    // first MapNotify event, so it never starts an open/close animation.
    let window_bypass = window.downgrade();
    window.connect_realize(move |_| {
        if let Some(win) = window_bypass.upgrade() {
            suppress_x11_compositor_animation(&win);
        }
    });

    // Show the window
    let _ = window.grab_focus();
    window.present();
}
