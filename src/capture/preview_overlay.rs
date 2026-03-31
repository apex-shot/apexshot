use crate::config::load_config;
use gdk4x11::X11Surface;
use gtk4::gdk::Key;
use gtk4::{
    gdk,
    glib::{self, ControlFlow},
    prelude::*,
    Align, Box as GtkBox, Button, CssProvider, DragSource, DrawingArea, EventControllerKey,
    EventControllerMotion, Label, Orientation, Overlay, WidgetPaintable, Window,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use x11rb::wrapper::ConnectionExt;
use x11rb::{
    connection::Connection,
    protocol::xproto::{self, ConnectionExt as _},
};

/// Generate a unique preview ID based on PID and current timestamp (milliseconds).
fn generate_preview_id(pid: u32) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("preview-{}-{}", pid, ts)
}

const PREVIEW_WIDTH: i32 = 211;
const PREVIEW_HEIGHT: i32 = 151;
const PREVIEW_EDGE_MARGIN: i32 = 24;
const PREVIEW_BOTTOM_SAFE_OFFSET: i32 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewDismissAction {
    Close,
    Hide,
}

fn preview_dimensions(scale: f64) -> (i32, i32) {
    let width = ((PREVIEW_WIDTH as f64) * scale).round() as i32;
    let height = ((PREVIEW_HEIGHT as f64) * scale).round() as i32;
    (width.max(1), height.max(1))
}

fn preview_side(position: &str) -> PreviewSide {
    match position {
        "Right" => PreviewSide::Right,
        _ => PreviewSide::Left,
    }
}

fn should_emit_extension_events(multi_display: bool, layer_shell_active: bool) -> bool {
    multi_display && !layer_shell_active
}

fn initial_preview_pinned(auto_close_enabled: bool) -> bool {
    !auto_close_enabled
}

fn preview_dismiss_action(action: &str) -> PreviewDismissAction {
    match action {
        "Hide" => PreviewDismissAction::Hide,
        _ => PreviewDismissAction::Close,
    }
}

fn should_dismiss_for_behavior(currently_pinned: bool, behavior_enabled: bool) -> bool {
    behavior_enabled || !currently_pinned
}

#[derive(Debug, Error)]
pub enum CapturePreviewError {
    #[error("Screenshot file not found: {0}")]
    MissingFile(PathBuf),

    #[error("Failed to convert screenshot path to file URI")]
    InvalidPath,

    #[error("Clipboard tool not found (install wl-clipboard or xclip)")]
    ClipboardToolNotFound,

    #[error("Clipboard command failed")]
    ClipboardCommandFailed,

    #[error("Failed to open target: {0}")]
    OpenTargetError(String),

    #[error("Failed to open editor: {0}")]
    EditorOpenError(String),
}

pub fn show_capture_preview_overlay(path: PathBuf) -> Result<(), CapturePreviewError> {
    if !path.exists() {
        return Err(CapturePreviewError::MissingFile(path));
    }

    if let Err(err) = gtk4::init() {
        eprintln!("Preview GTK init warning: {err}");
    }

    unsafe {
        std::env::set_var("DESKTOP_STARTUP_ID", "");
    }

    let pid = std::process::id();
    let preview_id = generate_preview_id(pid);

    let main_loop = glib::MainLoop::new(None, false);
    setup_preview_window(&main_loop, path, preview_id);
    main_loop.run();
    Ok(())
}

fn setup_preview_window(main_loop: &glib::MainLoop, path: PathBuf, preview_id: String) {
    install_preview_css();
    let config = load_config();
    let side = preview_side(&config.quick_access_position);
    let (preview_width, preview_height) = preview_dimensions(config.quick_access_overlay_size);
    let dismiss_action = preview_dismiss_action(&config.quick_access_auto_close_action);
    let dismiss_after_dragging = config.quick_access_close_after_dragging;
    let dismiss_after_uploading = config.quick_access_close_after_uploading;
    let start_pinned = initial_preview_pinned(config.quick_access_auto_close_enabled);
    let auto_close_seconds = config.quick_access_auto_close_interval as u64;

    let window = Window::builder()
        .title("Screenshot")
        .default_width(preview_width)
        .default_height(preview_height)
        .resizable(false)
        .decorated(false)
        .build();
    window.add_css_class("capture-preview-window");
    let layer_shell_active = configure_window_positioning(&window, side, preview_width);
    let limited_always_on_top = should_warn_about_wayland_topmost(layer_shell_active);
    if limited_always_on_top {
        window.add_css_class("capture-preview-window-limited");
    }
    // Intentionally silent when layer-shell is unavailable — the fallback
    // (bottom-left placement via X11 input-region) works correctly on X11
    // and non-layer-shell Wayland compositors. Logging this at startup every
    // time a preview appears creates unnecessary noise in system journals.

    let emit_extension_events =
        should_emit_extension_events(config.quick_access_multi_display, layer_shell_active);

    let pinned = Arc::new(AtomicBool::new(start_pinned));
    let edit_opened = Arc::new(AtomicBool::new(false));
    let auto_close_anchor = Arc::new(Mutex::new(Instant::now()));
    let source_bytes = Arc::new(Mutex::new(None::<Arc<Vec<u8>>>));

    let preview_area = build_preview_area(path.clone(), preview_width, preview_height);
    preview_area.set_widget_name("capture-preview-image");

    let card = Overlay::new();
    card.set_widget_name("capture-preview-card");
    card.set_overflow(gtk4::Overflow::Hidden);
    card.set_size_request(preview_width, preview_height);
    card.set_hexpand(false);
    card.set_vexpand(false);
    card.set_child(Some(&preview_area));

    let hover_tint = GtkBox::new(Orientation::Vertical, 0);
    hover_tint.set_widget_name("capture-preview-hover-tint");
    hover_tint.set_hexpand(true);
    hover_tint.set_vexpand(true);
    hover_tint.set_can_target(false);
    hover_tint.set_visible(false);

    let (pin_btn, pin_icon) = icon_button("view-pin-symbolic", "Pin");
    let (close_btn, _) = icon_button("window-close-symbolic", "Close");
    let copy_btn = Button::with_label("Copy");
    copy_btn.set_has_frame(false);
    copy_btn.set_focusable(false);
    copy_btn.add_css_class("preview-pill-action");

    let save_btn = Button::with_label("Save");
    save_btn.set_has_frame(false);
    save_btn.set_focusable(false);
    save_btn.add_css_class("preview-pill-action");
    let (edit_btn, _) = icon_button("document-edit-symbolic", "Edit");
    let (upload_btn, _) = icon_button("document-send-symbolic", "Upload");

    let top_controls = GtkBox::new(Orientation::Horizontal, 0);
    top_controls.add_css_class("preview-controls");
    top_controls.set_halign(Align::Fill);
    top_controls.set_valign(Align::Start);
    top_controls.set_margin_top(4);
    top_controls.set_margin_start(4);
    top_controls.set_margin_end(4);

    let top_spacer = GtkBox::new(Orientation::Horizontal, 0);
    top_spacer.set_hexpand(true);
    top_controls.append(&pin_btn);
    top_controls.append(&top_spacer);
    top_controls.append(&close_btn);

    let center_controls = GtkBox::new(Orientation::Vertical, 10);
    center_controls.add_css_class("preview-controls");
    center_controls.set_halign(Align::Center);
    center_controls.set_valign(Align::Center);
    center_controls.append(&copy_btn);
    center_controls.append(&save_btn);

    let bottom_controls = GtkBox::new(Orientation::Horizontal, 0);
    bottom_controls.add_css_class("preview-controls");
    bottom_controls.set_halign(Align::Fill);
    bottom_controls.set_valign(Align::End);
    bottom_controls.set_margin_bottom(4);
    bottom_controls.set_margin_start(4);
    bottom_controls.set_margin_end(4);

    let bottom_spacer = GtkBox::new(Orientation::Horizontal, 0);
    bottom_spacer.set_hexpand(true);
    bottom_controls.append(&edit_btn);
    bottom_controls.append(&bottom_spacer);
    bottom_controls.append(&upload_btn);

    let fallback_notice = Label::new(None);
    fallback_notice.add_css_class("preview-warning-badge");
    fallback_notice.set_visible(false);

    top_controls.set_visible(false);
    center_controls.set_visible(false);
    bottom_controls.set_visible(false);

    card.add_overlay(&hover_tint);
    card.add_overlay(&fallback_notice);
    card.add_overlay(&top_controls);
    card.add_overlay(&center_controls);
    card.add_overlay(&bottom_controls);

    if layer_shell_active {
        window.set_child(Some(&card));
    } else {
        // Keep a monitor-sized transparent fallback surface so the card can stay
        // bottom-left even when layer-shell is unavailable.
        let (fallback_width, fallback_height) = gdk::Display::default()
            .map(|display| {
                let monitors = display.monitors();
                let mut min_x = i32::MAX;
                let mut min_y = i32::MAX;
                let mut max_x = i32::MIN;
                let mut max_y = i32::MIN;

                for i in 0..monitors.n_items() {
                    if let Some(obj) = monitors.item(i) {
                        if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                            let geometry = monitor.geometry();
                            min_x = min_x.min(geometry.x());
                            min_y = min_y.min(geometry.y());
                            max_x = max_x.max(geometry.x() + geometry.width());
                            max_y = max_y.max(geometry.y() + geometry.height());
                        }
                    }
                }

                if min_x == i32::MAX || min_y == i32::MAX || max_x == i32::MIN || max_y == i32::MIN
                {
                    (1280, 720)
                } else {
                    ((max_x - min_x).max(1), (max_y - min_y).max(1))
                }
            })
            .unwrap_or((1280, 720));

        let fallback_window_width = fallback_width.max(preview_width + PREVIEW_EDGE_MARGIN * 2);
        let fallback_window_height = fallback_height
            .max(preview_height + (PREVIEW_EDGE_MARGIN * 2) + PREVIEW_BOTTOM_SAFE_OFFSET);
        window.set_default_size(fallback_window_width, fallback_window_height);

        let fallback_shell = Overlay::new();
        fallback_shell.set_widget_name("capture-preview-fallback-shell");
        fallback_shell.set_hexpand(true);
        fallback_shell.set_vexpand(true);
        fallback_shell.set_halign(Align::Fill);
        fallback_shell.set_valign(Align::Fill);

        let fallback_backdrop = GtkBox::new(Orientation::Vertical, 0);
        fallback_backdrop.set_hexpand(true);
        fallback_backdrop.set_vexpand(true);
        fallback_backdrop.set_size_request(fallback_window_width, fallback_window_height);
        fallback_shell.set_child(Some(&fallback_backdrop));
        fallback_shell.set_size_request(fallback_window_width, fallback_window_height);

        card.set_halign(match side {
            PreviewSide::Left => Align::Start,
            PreviewSide::Right => Align::End,
        });
        card.set_valign(Align::End);
        card.set_margin_start(if side == PreviewSide::Left {
            PREVIEW_EDGE_MARGIN
        } else {
            0
        });
        card.set_margin_end(if side == PreviewSide::Right {
            PREVIEW_EDGE_MARGIN
        } else {
            0
        });
        card.set_margin_top(PREVIEW_EDGE_MARGIN);
        card.set_margin_bottom(PREVIEW_EDGE_MARGIN + PREVIEW_BOTTOM_SAFE_OFFSET);
        fallback_shell.add_overlay(&card);
        fallback_shell.set_measure_overlay(&card, false);

        window.set_child(Some(&fallback_shell));
    }

    let use_fallback_input_region = !layer_shell_active;

    if use_fallback_input_region {
        let window_fallback_region = window.downgrade();
        let card_fallback_region = card.downgrade();
        window.connect_map(move |_| {
            let window_fallback_region = window_fallback_region.clone();
            let card_fallback_region = card_fallback_region.clone();
            glib::idle_add_local_once(move || {
                if let (Some(window), Some(card)) = (
                    window_fallback_region.upgrade(),
                    card_fallback_region.upgrade(),
                ) {
                    apply_fallback_input_region(&window, &card);
                }
            });
        });

        let window_fallback_stacking = window.downgrade();
        window.connect_map(move |_| {
            let window_fallback_stacking = window_fallback_stacking.clone();
            glib::idle_add_local_once(move || {
                if let Some(window) = window_fallback_stacking.upgrade() {
                    if let Err(err) = request_x11_always_on_top(&window) {
                        if !is_non_x11_surface_error(&err) {
                            eprintln!(
                                "Preview fallback warning: failed to enable always-on-top persistence: {err}"
                            );
                        }
                    }
                }
            });
        });

        let window_fallback_reassert = window.downgrade();
        window.connect_is_active_notify(move |_| {
            if let Some(window) = window_fallback_reassert.upgrade() {
                if let Err(err) = request_x11_always_on_top(&window) {
                    if !is_non_x11_surface_error(&err) {
                        eprintln!(
                            "Preview fallback warning: failed to reassert always-on-top state: {err}"
                        );
                    }
                }
            }
        });

        let window_fallback_watchdog = window.downgrade();
        glib::timeout_add_seconds_local(2, move || {
            let Some(window) = window_fallback_watchdog.upgrade() else {
                return ControlFlow::Break;
            };

            if let Err(err) = request_x11_always_on_top(&window) {
                if is_non_x11_surface_error(&err) {
                    return ControlFlow::Break;
                }

                eprintln!(
                    "Preview fallback warning: periodic always-on-top reassert failed: {err}"
                );
            }

            ControlFlow::Continue
        });
    }

    let motion = EventControllerMotion::new();
    let card_hover_enter = card.clone();
    let hover_tint_enter = hover_tint.clone();
    let top_controls_enter = top_controls.clone();
    let center_controls_enter = center_controls.clone();
    let bottom_controls_enter = bottom_controls.clone();
    motion.connect_enter(move |_, _, _| {
        card_hover_enter.add_css_class("preview-card-hover");
        hover_tint_enter.set_visible(true);
        top_controls_enter.set_visible(true);
        center_controls_enter.set_visible(true);
        bottom_controls_enter.set_visible(true);
    });

    let card_hover_leave = card.clone();
    let hover_tint_leave = hover_tint.clone();
    let top_controls_leave = top_controls.clone();
    let center_controls_leave = center_controls.clone();
    let bottom_controls_leave = bottom_controls.clone();
    motion.connect_leave(move |_| {
        card_hover_leave.remove_css_class("preview-card-hover");
        hover_tint_leave.set_visible(false);
        top_controls_leave.set_visible(false);
        center_controls_leave.set_visible(false);
        bottom_controls_leave.set_visible(false);
    });
    card.add_controller(motion);

    let uri = match file_uri(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to enable drag/drop for preview: {e}");
            window.present();
            return;
        }
    };

    let uri_provider = gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from_owned(format!("{uri}\r\n").into_bytes()),
    );
    let text_provider = gdk::ContentProvider::for_value(&uri.to_value());
    let provider = gdk::ContentProvider::new_union(&[uri_provider, text_provider]);

    let drag_source = DragSource::new();
    drag_source.set_actions(gdk::DragAction::COPY);
    drag_source.set_content(Some(&provider));
    let provider_prepare = provider.clone();
    drag_source.connect_prepare(move |_, _, _| Some(provider_prepare.clone()));
    let drag_paintable = WidgetPaintable::new(Some(&card));
    drag_source.set_icon(Some(&drag_paintable), 24, 24);

    let window_weak_drag = window.downgrade();
    let pinned_drag = pinned.clone();
    let edit_opened_drag = edit_opened.clone();
    let drag_dismiss_action = dismiss_action;
    drag_source.connect_drag_end(move |_, _, _| {
        if edit_opened_drag.load(Ordering::Relaxed) {
            return;
        }
        if !should_dismiss_for_behavior(pinned_drag.load(Ordering::Relaxed), dismiss_after_dragging)
        {
            return;
        }
        if let Some(window) = window_weak_drag.upgrade() {
            dismiss_preview_window(&window, drag_dismiss_action);
        }
    });
    card.add_controller(drag_source);

    let window_weak_close = window.downgrade();
    close_btn.connect_clicked(move |_| {
        if let Some(window) = window_weak_close.upgrade() {
            window.close();
        }
    });

    let pin_state = pinned.clone();
    let auto_close_anchor_pin = auto_close_anchor.clone();
    if start_pinned {
        pin_icon.set_icon_name(Some("starred-symbolic"));
    }
    pin_btn.connect_clicked(move |_| {
        let now_pinned = !pin_state.load(Ordering::Relaxed);
        pin_state.store(now_pinned, Ordering::Relaxed);

        if !now_pinned {
            if let Ok(mut anchor) = auto_close_anchor_pin.lock() {
                *anchor = Instant::now();
            }
        }

        // Swap pin icon to reflect pinned state
        if now_pinned {
            pin_icon.set_icon_name(Some("starred-symbolic"));
        } else {
            pin_icon.set_icon_name(Some("view-pin-symbolic"));
        }
    });

    let path_copy = path.clone();
    copy_btn.connect_clicked(move |_| {
        if let Err(e) = copy_uri_to_clipboard(&path_copy) {
            eprintln!("Copy failed: {e}");
        }
    });

    let window_weak_save = window.downgrade();
    save_btn.connect_clicked(move |_| {
        if let Some(window) = window_weak_save.upgrade() {
            window.close();
        }
    });

    let path_edit = path.clone();
    let source_bytes_edit = source_bytes.clone();
    let edit_opened_btn = edit_opened.clone();
    let window_weak_edit = window.downgrade();
    edit_btn.connect_clicked(move |_| {
        if !path_edit.exists() {
            let cached_bytes = source_bytes_edit
                .lock()
                .ok()
                .and_then(|guard| guard.clone());
            if let Some(bytes) = cached_bytes {
                if let Err(e) = std::fs::write(&path_edit, bytes.as_slice()) {
                    eprintln!("Edit failed: could not restore missing screenshot file: {e}");
                    return;
                }
            } else {
                eprintln!("Edit failed: screenshot path no longer exists");
                return;
            }
        }

        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
        if let Err(e) = Command::new(&exe).arg("edit").arg(&path_edit).spawn() {
            eprintln!("Edit failed: {e}");
            return;
        }

        edit_opened_btn.store(true, Ordering::Relaxed);

        if let Some(window) = window_weak_edit.upgrade() {
            window.close();
        }
    });

    let path_upload = path.clone();
    let window_weak_upload = window.downgrade();
    let pinned_upload = pinned.clone();
    let upload_dismiss_action = dismiss_action;
    upload_btn.connect_clicked(move |_| {
        let target = path_upload
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path_upload.clone());
        if let Err(e) = open_target(&target) {
            eprintln!("Upload action failed: {e}");
            return;
        }

        if !should_dismiss_for_behavior(
            pinned_upload.load(Ordering::Relaxed),
            dismiss_after_uploading,
        ) {
            return;
        }

        if let Some(window) = window_weak_upload.upgrade() {
            dismiss_preview_window(&window, upload_dismiss_action);
        }
    });

    let key_controller = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let window_weak_esc = window.downgrade();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == Key::Escape {
            if let Some(window) = window_weak_esc.upgrade() {
                window.close();
            }
            return glib::Propagation::Stop;
        }
        if key == Key::Return || key == Key::KP_Enter || key == Key::space {
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let window_weak_timeout = window.downgrade();
    let pinned_timeout = pinned.clone();
    let edit_opened_timeout = edit_opened.clone();
    let auto_close_anchor_timeout = auto_close_anchor.clone();
    let timeout_dismiss_action = dismiss_action;
    glib::timeout_add_seconds_local(1, move || {
        if edit_opened_timeout.load(Ordering::Relaxed) {
            return ControlFlow::Break;
        }

        let auto_close_elapsed = auto_close_anchor_timeout
            .lock()
            .map(|anchor| anchor.elapsed().as_secs())
            .unwrap_or(0);

        if !pinned_timeout.load(Ordering::Relaxed) && auto_close_elapsed >= auto_close_seconds {
            if let Some(window) = window_weak_timeout.upgrade() {
                dismiss_preview_window(&window, timeout_dismiss_action);
            }
            return ControlFlow::Break;
        }

        ControlFlow::Continue
    });

    let main_loop_close = main_loop.clone();
    let edit_opened_close = edit_opened.clone();
    let preview_id_close = preview_id.clone();
    window.connect_close_request(move |_| {
        if !edit_opened_close.load(Ordering::Relaxed) {
            main_loop_close.quit();
        }
        if emit_extension_events {
            crate::gnome_integration::emit_preview_closed(&preview_id_close);
        }
        glib::Propagation::Proceed
    });

    // On X11: set the window-type hint as soon as the native window is
    // realized (XID assigned) but BEFORE it is mapped/shown.  This ensures
    // the compositor sees _NET_WM_WINDOW_TYPE_NOTIFICATION on the very first
    // MapNotify event and never starts an open/close animation.
    let window_type_hint = window.downgrade();
    window.connect_realize(move |_| {
        if let Some(win) = window_type_hint.upgrade() {
            suppress_x11_preview_window_type(&win);
        }
    });

    let path_source_bytes = path.clone();
    let source_bytes_cache = source_bytes.clone();
    glib::idle_add_local_once(move || {
        if let Ok(bytes) = std::fs::read(&path_source_bytes) {
            if let Ok(mut cache) = source_bytes_cache.lock() {
                *cache = Some(Arc::new(bytes));
            }
        }
    });

    window.present();

    // Emit PreviewOpened with structured metadata so the GNOME extension can
    // track this preview by preview_id and match the Wayland window by PID.
    // Skip this when layer_shell_active is true because:
    // 1. Layer-shell Overlay already keeps the window above everything
    // 2. Layer-shell surfaces are not exposed as MetaWindow, so the extension can't find it
    if emit_extension_events {
        let pid = std::process::id();
        crate::gnome_integration::emit_preview_opened(
            &preview_id,
            pid,
            "Screenshot",
            "apexshot-capture-preview",
        );
    }

    if let Some(surface) = window.surface() {
        if let Ok(_x11_surface) = surface.downcast::<X11Surface>() {
            // On X11 the extension is not used; no additional signal needed.
        }
    }
}

/// On X11, set `_NET_WM_WINDOW_TYPE_NOTIFICATION` so the preview card:
///  - does **not** appear in the taskbar / dock
///  - is **not** animated by the compositor (no slide-in / scale-up)
///  - stays above other windows without needing always-on-top tricks
///
/// Called from `connect_realize` so the hints are in place before the
/// first MapNotify — the compositor therefore never starts an animation.
fn suppress_x11_preview_window_type(window: &Window) {
    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(x11_surface) = surface.downcast::<X11Surface>() else {
        return; // Wayland path — layer-shell already handles this correctly
    };
    let Ok(xid) = u32::try_from(x11_surface.xid()) else {
        return;
    };
    let Ok((conn, _)) = x11rb::connect(None) else {
        return;
    };

    // _NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_NOTIFICATION
    // Notification windows skip the taskbar and compositor open/close animations.
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

    // _NET_WM_BYPASS_COMPOSITOR = 1 — ask the compositor to skip compositing
    // this window so it appears without any fade / scale animation.
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

fn install_preview_css() {
    if let Some(display) = gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(
            "
            .capture-preview-window {
                background: transparent;
                transition: none;
                transition-duration: 0s;
                animation: none;
                animation-duration: 0s;
            }

            .capture-preview-window,
            .capture-preview-window:backdrop {
                background-color: transparent;
                background-image: none;
                box-shadow: none;
            }

            window.capture-preview-window,
            window.capture-preview-window:backdrop,
            window.capture-preview-window > * {
                background-color: transparent;
                background-image: none;
                box-shadow: none;
                transition: none;
                transition-duration: 0s;
                animation: none;
                animation-duration: 0s;
            }

            #capture-preview-fallback-shell,
            #capture-preview-fallback-shell > * {
                background-color: transparent;
                background-image: none;
                box-shadow: none;
            }

            #capture-preview-card {
                border-radius: 10px;
                border: 1px solid alpha(white, 0.26);
                background: alpha(black, 0.12);
                box-shadow: 0 14px 42px alpha(black, 0.45), inset 0 1px 0 alpha(white, 0.14);
                outline-width: 0;
            }

            #capture-preview-card.preview-card-hover {
                border-color: alpha(white, 0.58);
                background: alpha(white, 0.10);
                box-shadow: 0 18px 54px alpha(black, 0.54), inset 0 1px 0 alpha(white, 0.20);
            }

            .capture-preview-window-limited #capture-preview-card {
                border-color: rgba(255, 197, 92, 0.82);
                box-shadow: 0 14px 42px alpha(black, 0.45), inset 0 0 0 1px rgba(255, 197, 92, 0.24);
            }

            label.preview-warning-badge {
                padding: 3px 10px;
                border-radius: 999px;
                border: 1px solid rgba(255, 222, 152, 0.68);
                background: rgba(44, 28, 8, 0.88);
                color: rgba(255, 238, 203, 0.98);
                font-size: 10px;
                font-weight: 700;
                letter-spacing: 0.02em;
                box-shadow: 0 8px 20px rgba(0, 0, 0, 0.24);
            }

            #capture-preview-hover-tint {
                border-radius: 10px;
                background: rgba(0, 0, 0, 0.52);
                box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.18),
                            0 3px 8px rgba(0, 0, 0, 0.30);
            }

            #capture-preview-image {
                border-radius: 10px;
            }

            .preview-controls {
                opacity: 0.98;
            }

            button.preview-action {
                min-width: 14px;
                min-height: 14px;
                padding: 1px;
                border-radius: 999px;
                border: 1px solid rgba(220, 220, 220, 1.0);
                background: white;
                color: rgba(20, 20, 20, 0.98);
                box-shadow: none;
                background-image: none;
                outline-width: 0;
            }

            button.preview-action:hover {
                border-color: rgba(160, 160, 160, 1.0);
                background: rgba(240, 240, 240, 1.0);
                color: rgba(8, 8, 8, 1.00);
                box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18);
                transform: scale(1.12);
            }

            button.preview-action:active {
                background: rgba(220, 220, 220, 1.0);
                transform: scale(0.96);
                box-shadow: none;
            }

            button.preview-action:focus,
            button.preview-action:focus-visible {
                outline: none;
                box-shadow: none;
            }

            button.preview-pill-action {
                min-width: 55px;
                min-height: 26px;
                padding: 0px 6px;
                border-radius: 999px;
                border: 1px solid alpha(white, 0.64);
                background: alpha(white, 0.80);
                color: rgba(18, 18, 18, 0.98);
                font-size: 11px;
                font-weight: 600;
                box-shadow: 0 6px 20px alpha(black, 0.20);
                background-image: none;
                outline-width: 0;
            }

            button.preview-pill-action:hover {
                border-color: alpha(white, 0.80);
                background: alpha(white, 0.96);
                color: rgba(10, 10, 10, 1.00);
            }

            button.preview-pill-action:active {
                background: alpha(white, 0.78);
            }

            button.preview-pill-action:focus,
            button.preview-pill-action:focus-visible {
                outline: none;
                box-shadow: 0 6px 20px alpha(black, 0.20);
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

fn icon_button(icon_name: &str, _tooltip: &str) -> (Button, gtk4::Image) {
    let image = gtk4::Image::from_icon_name(icon_name);
    image.set_pixel_size(12);

    let button = Button::new();
    button.set_child(Some(&image));
    button.set_has_frame(false);
    button.set_focusable(false);
    button.add_css_class("preview-action");

    (button, image)
}

fn env_var_contains_case_insensitive(name: &str, needle: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        })
        .unwrap_or(false)
}

fn is_gnome_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && (env_var_contains_case_insensitive("XDG_CURRENT_DESKTOP", "gnome")
            || env_var_contains_case_insensitive("DESKTOP_SESSION", "gnome")
            || std::env::var_os("GNOME_DESKTOP_SESSION_ID").is_some())
}

fn should_warn_about_wayland_topmost(layer_shell_active: bool) -> bool {
    !layer_shell_active && is_gnome_wayland_session()
}

fn file_uri(path: &Path) -> Result<String, CapturePreviewError> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| CapturePreviewError::InvalidPath)
}

fn copy_uri_to_clipboard(path: &Path) -> Result<(), CapturePreviewError> {
    let uri = file_uri(path)?;
    let payload = format!("{uri}\r\n");

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut child = Command::new("wl-copy")
            .arg("--type")
            .arg("text/uri-list")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CapturePreviewError::ClipboardToolNotFound
                } else {
                    CapturePreviewError::ClipboardCommandFailed
                }
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(payload.as_bytes())
                .map_err(|_| CapturePreviewError::ClipboardCommandFailed)?;
        }

        if child
            .wait()
            .map_err(|_| CapturePreviewError::ClipboardCommandFailed)?
            .success()
        {
            return Ok(());
        }

        return Err(CapturePreviewError::ClipboardCommandFailed);
    }

    let mut child = Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .arg("-t")
        .arg("text/uri-list")
        .arg("-i")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CapturePreviewError::ClipboardToolNotFound
            } else {
                CapturePreviewError::ClipboardCommandFailed
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.as_bytes())
            .map_err(|_| CapturePreviewError::ClipboardCommandFailed)?;
    }

    if child
        .wait()
        .map_err(|_| CapturePreviewError::ClipboardCommandFailed)?
        .success()
    {
        Ok(())
    } else {
        Err(CapturePreviewError::ClipboardCommandFailed)
    }
}

fn open_target(path: &Path) -> Result<(), CapturePreviewError> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| CapturePreviewError::OpenTargetError(e.to_string()))
}

fn apply_fallback_input_region(window: &Window, card: &Overlay) {
    let Some(surface) = window.surface() else {
        return;
    };

    let allocation = card.allocation();
    if allocation.width() <= 0 || allocation.height() <= 0 {
        return;
    }

    let region_rect = gtk4::cairo::RectangleInt::new(
        allocation.x(),
        allocation.y(),
        allocation.width(),
        allocation.height(),
    );
    let input_region = gtk4::cairo::Region::create_rectangle(&region_rect);
    surface.set_input_region(&input_region);
}

fn is_non_x11_surface_error(err: &str) -> bool {
    err.contains("surface is not X11")
}

fn request_x11_always_on_top(window: &Window) -> Result<(), String> {
    let surface = window
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

fn build_preview_area(path: PathBuf, preview_width: i32, preview_height: i32) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_size_request(preview_width, preview_height);
    area.set_hexpand(false);
    area.set_vexpand(false);
    area.set_can_target(true);

    let texture = Rc::new(RefCell::new(None::<gdk::Texture>));
    let texture_draw = texture.clone();
    area.set_draw_func(move |_area, cr, width, height| {
        let texture_ref = texture_draw.borrow();
        let Some(tex) = texture_ref.as_ref() else {
            return;
        };
        let tw = tex.width() as f64;
        let th = tex.height() as f64;
        if tw <= 0.0 || th <= 0.0 {
            return;
        }

        let scale = (width as f64 / tw).min(height as f64 / th);
        let sw = tw * scale;
        let sh = th * scale;
        let ox = (width as f64 - sw) / 2.0;
        let oy = (height as f64 - sh) / 2.0;

        let snapshot = gtk4::Snapshot::new();
        snapshot.translate(&gtk4::graphene::Point::new(ox as f32, oy as f32));
        tex.snapshot(&snapshot, sw, sh);
        if let Some(node) = snapshot.to_node() {
            node.draw(cr);
        }
    });

    let area_weak = area.downgrade();
    glib::idle_add_local_once(move || {
        let Some(area) = area_weak.upgrade() else {
            return;
        };
        *texture.borrow_mut() = preview_texture(&path, preview_width, preview_height);
        area.queue_draw();
    });

    area
}

fn preview_texture(path: &Path, preview_width: i32, preview_height: i32) -> Option<gdk::Texture> {
    let preview_pixbuf = match gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(
        path,
        preview_width,
        preview_height,
        true,
    ) {
        Ok(pixbuf) => pixbuf,
        Err(err) => {
            eprintln!(
                "Preview thumbnail warning: failed to read screenshot for overlay ({}).",
                err
            );
            return None;
        }
    };

    Some(gdk::Texture::for_pixbuf(&preview_pixbuf))
}

fn configure_window_positioning(window: &Window, side: PreviewSide, preview_width: i32) -> bool {
    if gtk4_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_namespace(Some("apexshot-capture-preview"));
        window.set_layer(Layer::Overlay);

        window.set_anchor(Edge::Left, side == PreviewSide::Left);
        window.set_anchor(Edge::Right, side == PreviewSide::Right);
        window.set_anchor(Edge::Top, false);
        window.set_anchor(Edge::Bottom, true);

        window.set_exclusive_zone(preview_width + PREVIEW_EDGE_MARGIN * 2);
        window.set_margin(
            Edge::Left,
            if side == PreviewSide::Left {
                PREVIEW_EDGE_MARGIN
            } else {
                0
            },
        );
        window.set_margin(
            Edge::Right,
            if side == PreviewSide::Right {
                PREVIEW_EDGE_MARGIN
            } else {
                0
            },
        );
        window.set_margin(Edge::Top, 0);
        window.set_margin(
            Edge::Bottom,
            PREVIEW_EDGE_MARGIN + PREVIEW_BOTTOM_SAFE_OFFSET,
        );

        window.set_keyboard_mode(KeyboardMode::OnDemand);
        return true;
    }

    false
}

fn dismiss_preview_window(window: &Window, action: PreviewDismissAction) {
    match action {
        // The preview is a standalone transient window; "Hide" currently maps to
        // the same lifecycle as close because there is no background controller
        // that can restore a hidden preview later.
        PreviewDismissAction::Close | PreviewDismissAction::Hide => window.close(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_dimensions_keep_current_size_at_midpoint() {
        assert_eq!(preview_dimensions(1.0), (PREVIEW_WIDTH, PREVIEW_HEIGHT));
    }

    #[test]
    fn preview_side_resolves_left_and_right() {
        assert_eq!(preview_side("Left"), PreviewSide::Left);
        assert_eq!(preview_side("Right"), PreviewSide::Right);
        assert_eq!(preview_side("Top"), PreviewSide::Left);
    }

    #[test]
    fn preview_extension_signals_follow_multi_display_setting() {
        assert!(should_emit_extension_events(true, false));
        assert!(!should_emit_extension_events(false, false));
        assert!(!should_emit_extension_events(true, true));
    }

    #[test]
    fn preview_starts_pinned_when_auto_close_is_disabled() {
        assert!(initial_preview_pinned(false));
        assert!(!initial_preview_pinned(true));
    }
}
