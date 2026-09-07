use crate::config::load_config;
use crate::i18n::t;
use gdk4x11::X11Surface;
use gtk4::gdk::Key;
use gtk4::{
    gdk,
    glib::{self, ControlFlow},
    prelude::*,
    Align, ApplicationWindow, Box as GtkBox, Button, CssProvider, DragSource, DrawingArea,
    EventControllerKey, Orientation, Overlay, WidgetPaintable, Window,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Once,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Messages from the upload worker back to the GTK main loop.
enum UploadUiEvent {
    /// Upload finished (success or failure). Re-enable the button; dismiss only on success when configured.
    Finished {
        dismiss: bool,
        share_url: Option<String>,
    },
}

const PREVIEW_TIMING_ENV: &str = "APEXSHOT_PREVIEW_TIMING";
const PREVIEW_PARENT_START_ENV: &str = "APEXSHOT_PREVIEW_PARENT_START_MS";
const PREVIEW_DISABLE_LAYER_SHELL_ENV: &str = "APEXSHOT_PREVIEW_DISABLE_LAYER_SHELL";

#[derive(Clone)]
struct PreviewStartupProbe {
    enabled: bool,
    startup: Instant,
    path: PathBuf,
    parent_start_ms: Option<u128>,
}

impl PreviewStartupProbe {
    fn from_env(path: PathBuf) -> Self {
        let enabled = std::env::var_os(PREVIEW_TIMING_ENV).is_some();
        let parent_start_ms = std::env::var(PREVIEW_PARENT_START_ENV)
            .ok()
            .and_then(|value| value.parse::<u128>().ok());

        Self {
            enabled,
            startup: Instant::now(),
            path,
            parent_start_ms,
        }
    }

    fn log(&self, stage: &str) {
        if !self.enabled {
            return;
        }

        let local_ms = self.startup.elapsed().as_millis();
        if let (Some(parent_start_ms), Ok(now)) = (
            self.parent_start_ms,
            SystemTime::now().duration_since(UNIX_EPOCH),
        ) {
            let total_ms = now.as_millis().saturating_sub(parent_start_ms);
            eprintln!(
                "[preview-timing] {} local={}ms total={}ms path={}",
                stage,
                local_ms,
                total_ms,
                self.path.display()
            );
        } else {
            eprintln!(
                "[preview-timing] {} local={}ms path={}",
                stage,
                local_ms,
                self.path.display()
            );
        }
    }
}

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

const PREVIEW_WIDTH: i32 = 190;
const PREVIEW_HEIGHT: i32 = 135;
const PREVIEW_EDGE_MARGIN: i32 = 24;
const PREVIEW_BOTTOM_SAFE_OFFSET: i32 = 80;
const PREVIEW_FRAME_INSET: i32 = 5;
const PREVIEW_CORNER_MARGIN: i32 = 6;
const PREVIEW_SHADOW_PAD: i32 = 10;

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

fn preview_chrome_padding() -> i32 {
    PREVIEW_FRAME_INSET * 2 + PREVIEW_SHADOW_PAD * 2
}

fn preview_side(position: &str) -> PreviewSide {
    match position {
        "Right" => PreviewSide::Right,
        _ => PreviewSide::Left,
    }
}

fn desktop_value_contains(desktop: Option<&str>, needle: &str) -> bool {
    desktop
        .unwrap_or_default()
        .split([':', ';', ','])
        .any(|part| part.trim().eq_ignore_ascii_case(needle))
}

fn is_gnome_wayland_session_from_env(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
    desktop_session: Option<&str>,
) -> bool {
    let is_wayland = wayland_display.is_some_and(|value| !value.trim().is_empty());
    let is_gnome = desktop_value_contains(desktop, "GNOME")
        || desktop_value_contains(desktop_session, "gnome");
    is_wayland && is_gnome
}

fn is_gnome_wayland_session() -> bool {
    is_gnome_wayland_session_from_env(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        std::env::var("DESKTOP_SESSION").ok().as_deref(),
    )
}

fn should_emit_extension_events(layer_shell_active: bool) -> bool {
    // Layer-shell surfaces are not MetaWindows, so the GNOME helper cannot
    // raise them. Regular windows always announce themselves.
    !layer_shell_active
}

const PREVIEW_TRACKED_TITLE: &str = "ApexShot Preview";
const PREVIEW_TRACKED_ROLE: &str = "preview";
const PREVIEW_TRACKED_NAMESPACE: &str = "apexshot-capture-preview";

fn preview_should_stay_on_top(pinned: bool) -> bool {
    pinned
}

fn apply_preview_stacking(
    window: Option<&Window>,
    preview_id: &str,
    emit_extension_events: bool,
    stay_on_top: bool,
) {
    if emit_extension_events {
        if stay_on_top {
            crate::gnome_integration::emit_tracked_window_opened(
                preview_id,
                std::process::id(),
                PREVIEW_TRACKED_TITLE,
                PREVIEW_TRACKED_ROLE,
                PREVIEW_TRACKED_NAMESPACE,
            );
        } else {
            crate::gnome_integration::emit_tracked_window_closed(preview_id);
        }
    }

    let Some(window) = window else {
        return;
    };
    if let Err(err) = request_x11_always_on_top(window, stay_on_top) {
        if stay_on_top && !is_non_x11_surface_error(&err) {
            eprintln!("Preview stacking warning: {err}");
        }
    }
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

/// Whether a successful Quick Access upload should dismiss the preview overlay.
fn should_close_preview_after_upload(close_after_uploading: bool) -> bool {
    close_after_uploading
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
    let probe = PreviewStartupProbe::from_env(path.clone());
    probe.log("preview-entry");

    if !path.exists() {
        return Err(CapturePreviewError::MissingFile(path));
    }

    probe.log("path-exists-confirmed");

    // Force-set GIO_LAUNCHED_DESKTOP_FILE to the main app's desktop entry.
    // This process may have been spawned by the daemon, which sets its own
    // GIO_LAUNCHED_DESKTOP_FILE pointing to the autostart daemon desktop file.
    // We MUST override it so GNOME Shell shows "ApexShot" with the correct icon
    // instead of "GTK Application" or "ApexShot Daemon".
    if let Some(desktop_path) = crate::app_identity::desktop_file_for_portal() {
        std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", desktop_path);
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE_PID",
            std::process::id().to_string(),
        );
    } else {
        let app_id = std::env::var("APEXSHOT_APP_ID")
            .unwrap_or_else(|_| crate::app_identity::app_id().to_string());
        if let Ok(desktop_path) = crate::hotkeys::ensure_desktop_entry_pub(&app_id) {
            std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", &desktop_path);
            std::env::set_var(
                "GIO_LAUNCHED_DESKTOP_FILE_PID",
                std::process::id().to_string(),
            );
        }
    }

    // Use the main app ID so GNOME Shell can find the desktop file and icon.
    // G_APPLICATION_NON_UNIQUE allows multiple processes with the same ID
    // (e.g. settings + preview running simultaneously).
    probe.log("before-gtk-application-build");
    let app = gtk4::Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    probe.log("after-gtk-application-build");

    let path_clone = path.clone();
    let probe_activate = probe.clone();
    app.connect_activate(move |app| {
        probe_activate.log("activate-enter");
        let pid = std::process::id();
        let preview_id = generate_preview_id(pid);
        probe_activate.log("before-setup-preview-window");

        setup_preview_window(app, &path_clone, preview_id, probe_activate.clone());
    });

    probe.log("before-app-run");
    app.run_with_args(&[""]);
    probe.log("after-app-run");
    Ok(())
}

fn setup_preview_window(
    app: &gtk4::Application,
    path: &Path,
    preview_id: String,
    probe: PreviewStartupProbe,
) {
    probe.log("setup-preview-window-enter");
    let startup = probe.startup;
    let path = path.to_path_buf();

    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(
            crate::capture::editor::window::icon_names::GRESOURCE_BYTES,
            crate::capture::editor::window::icon_names::RESOURCE_PREFIX,
        );
    });
    probe.log("before-install-preview-css");
    install_preview_css();
    probe.log("after-install-preview-css");
    probe.log("before-load-config");
    let config = load_config();
    probe.log("after-load-config");
    let side = preview_side(&config.quick_access_position);
    let (preview_width, preview_height) = preview_dimensions(config.quick_access_overlay_size);
    let chrome_pad = preview_chrome_padding();
    let dismiss_action = preview_dismiss_action(&config.quick_access_auto_close_action);
    let dismiss_after_dragging = config.quick_access_close_after_dragging;
    let start_pinned = initial_preview_pinned(config.quick_access_auto_close_enabled);
    let auto_close_seconds = config.quick_access_auto_close_interval as u64;

    probe.log("before-window-build");
    let appwin = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Preview"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(preview_width + chrome_pad)
        .default_height(preview_height + chrome_pad)
        .resizable(false)
        .decorated(false)
        .build();
    probe.log("after-window-build");
    // Upcast to Window for all the helper functions that expect &Window
    let window: Window = appwin.upcast();
    window.add_css_class("capture-preview-window");
    probe.log("before-configure-window-positioning");
    let layer_shell_active = configure_window_positioning(&window, side, preview_width);
    probe.log("after-configure-window-positioning");
    // Intentionally silent when layer-shell is unavailable — the fallback
    // (bottom-left placement via X11 input-region) works correctly on X11
    // and non-layer-shell Wayland compositors. Logging this at startup every
    // time a preview appears creates unnecessary noise in system journals.

    let emit_extension_events = should_emit_extension_events(layer_shell_active);

    let probe_map = probe.clone();
    window.connect_map(move |_| {
        probe_map.log("window-map");
    });

    let probe_realize = probe.clone();
    window.connect_realize(move |_| {
        probe_realize.log("window-realize");
    });

    // On X11: set the window-type hint as soon as the native window is
    // realized (XID assigned) but BEFORE it is mapped/shown. This ensures
    // the compositor sees _NET_WM_WINDOW_TYPE_NOTIFICATION on the very first
    // MapNotify event and never starts an open/close animation.
    let window_type_hint = window.downgrade();
    window.connect_realize(move |_| {
        if let Some(win) = window_type_hint.upgrade() {
            suppress_x11_preview_window_type(&win);
        }
    });

    let first_frame_seen = Rc::new(RefCell::new(false));
    let first_frame_probe = probe.clone();
    let first_frame_seen_tick = first_frame_seen.clone();
    window.add_tick_callback(move |widget, _| {
        if !*first_frame_seen_tick.borrow() {
            *first_frame_seen_tick.borrow_mut() = true;
            first_frame_probe.log("first-frame-tick");
            widget.queue_draw();
            return ControlFlow::Break;
        }
        ControlFlow::Break
    });

    let pinned = Arc::new(AtomicBool::new(start_pinned));
    let edit_opened = Arc::new(AtomicBool::new(false));
    let auto_close_anchor = Arc::new(Mutex::new(Instant::now()));
    let source_bytes = Arc::new(Mutex::new(None::<Arc<Vec<u8>>>));

    probe.log("before-build-preview-area");
    let preview_area = build_preview_area(
        path.to_path_buf(),
        preview_width,
        preview_height,
        startup,
        probe.clone(),
    );
    probe.log("after-build-preview-area");
    preview_area.set_widget_name("capture-preview-image");

    // Image frame: the screenshot sits inside with its own rounded corners
    let image_frame = GtkBox::new(Orientation::Vertical, 0);
    image_frame.set_widget_name("capture-preview-image-frame");
    image_frame.set_overflow(gtk4::Overflow::Hidden);
    image_frame.set_size_request(preview_width, preview_height);
    image_frame.set_margin_start(PREVIEW_FRAME_INSET);
    image_frame.set_margin_end(PREVIEW_FRAME_INSET);
    image_frame.set_margin_top(PREVIEW_FRAME_INSET);
    image_frame.set_margin_bottom(PREVIEW_FRAME_INSET);
    image_frame.append(&preview_area);

    let (close_btn, _) = corner_icon_button(
        crate::capture::editor::window::icon_names::DISMISS_REGULAR,
        &t("Close"),
        Align::Start,
        Align::Start,
    );
    close_btn.set_widget_name("preview-close-btn");
    close_btn.add_css_class("preview-close-btn");

    let (pin_btn, pin_icon) = corner_icon_button(
        crate::capture::editor::window::icon_names::VIEW_PIN,
        &t("Pin"),
        Align::End,
        Align::Start,
    );
    let (upload_btn, _) = corner_icon_button(
        crate::capture::editor::window::icon_names::ARROW_EXPORT_UP_REGULAR,
        &t("Upload to cloud"),
        Align::Start,
        Align::End,
    );
    let (edit_btn, _) = corner_icon_button(
        crate::capture::editor::window::icon_names::custom::PENCIL_SYMBOLIC,
        &t("Edit"),
        Align::End,
        Align::End,
    );
    let copy_btn = copy_pill_button(&t("Copy"));

    // Framed card: screenshot with corner actions and a centered Copy pill.
    let card = Overlay::new();
    card.set_widget_name("capture-preview-card");
    card.set_hexpand(false);
    card.set_vexpand(false);
    card.set_child(Some(&image_frame));
    card.add_overlay(&close_btn);
    card.add_overlay(&pin_btn);
    card.add_overlay(&upload_btn);
    card.add_overlay(&edit_btn);
    card.add_overlay(&copy_btn);
    card.set_measure_overlay(&close_btn, false);
    card.set_measure_overlay(&pin_btn, false);
    card.set_measure_overlay(&upload_btn, false);
    card.set_measure_overlay(&edit_btn, false);
    card.set_measure_overlay(&copy_btn, false);

    let chrome = GtkBox::new(Orientation::Vertical, 0);
    chrome.set_widget_name("capture-preview-chrome");
    chrome.set_hexpand(false);
    chrome.set_vexpand(false);
    chrome.append(&card);

    probe.log("after-card-assembly");

    probe.log("before-window-child-setup");
    if layer_shell_active {
        window.set_child(Some(&chrome));
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

        let fallback_window_width =
            fallback_width.max(preview_width + chrome_pad + PREVIEW_EDGE_MARGIN * 2);
        let fallback_window_height = fallback_height.max(
            preview_height + chrome_pad + (PREVIEW_EDGE_MARGIN * 2) + PREVIEW_BOTTOM_SAFE_OFFSET,
        );
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

        chrome.set_halign(match side {
            PreviewSide::Left => Align::Start,
            PreviewSide::Right => Align::End,
        });
        chrome.set_valign(Align::End);
        chrome.set_margin_start(if side == PreviewSide::Left {
            PREVIEW_EDGE_MARGIN
        } else {
            0
        });
        chrome.set_margin_end(if side == PreviewSide::Right {
            PREVIEW_EDGE_MARGIN
        } else {
            0
        });
        chrome.set_margin_top(PREVIEW_EDGE_MARGIN);
        chrome.set_margin_bottom(PREVIEW_EDGE_MARGIN + PREVIEW_BOTTOM_SAFE_OFFSET);
        fallback_shell.add_overlay(&chrome);
        fallback_shell.set_measure_overlay(&chrome, false);

        window.set_child(Some(&fallback_shell));
    }
    probe.log("after-window-child-setup");

    probe.log("before-window-present");
    window.present();
    probe.log("after-window-present");

    let use_fallback_input_region = !layer_shell_active;
    install_fallback_input_region_tracking(&window, &card);

    if use_fallback_input_region {
        let window_fallback_stacking = window.downgrade();
        let pinned_map = pinned.clone();
        window.connect_map(move |_| {
            if !preview_should_stay_on_top(pinned_map.load(Ordering::Relaxed)) {
                return;
            }
            let window_fallback_stacking = window_fallback_stacking.clone();
            glib::idle_add_local_once(move || {
                if let Some(window) = window_fallback_stacking.upgrade() {
                    if let Err(err) = request_x11_always_on_top(&window, true) {
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
        let pinned_reassert = pinned.clone();
        window.connect_is_active_notify(move |_| {
            if !preview_should_stay_on_top(pinned_reassert.load(Ordering::Relaxed)) {
                return;
            }
            if let Some(window) = window_fallback_reassert.upgrade() {
                if let Err(err) = request_x11_always_on_top(&window, true) {
                    if !is_non_x11_surface_error(&err) {
                        eprintln!(
                            "Preview fallback warning: failed to reassert always-on-top state: {err}"
                        );
                    }
                }
            }
        });

        let window_fallback_watchdog = window.downgrade();
        let pinned_watchdog = pinned.clone();
        glib::timeout_add_seconds_local(2, move || {
            let Some(window) = window_fallback_watchdog.upgrade() else {
                return ControlFlow::Break;
            };
            if !preview_should_stay_on_top(pinned_watchdog.load(Ordering::Relaxed)) {
                return ControlFlow::Continue;
            }

            if let Err(err) = request_x11_always_on_top(&window, true) {
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

    let path_actions = path.clone();
    let window_actions = window.downgrade();
    let card_actions = card.clone();
    let pinned_actions = pinned.clone();
    let edit_opened_actions = edit_opened.clone();
    let auto_close_anchor_actions = auto_close_anchor.clone();
    let source_bytes_actions = source_bytes.clone();
    let pin_icon_actions = pin_icon.clone();
    let edit_btn_actions = edit_btn.clone();
    let copy_btn_actions = copy_btn.clone();
    let upload_btn_actions = upload_btn.clone();
    let close_btn_actions = close_btn.clone();
    let pin_btn_actions = pin_btn.clone();
    let startup_actions = startup;
    let dismiss_action_actions = dismiss_action;
    let start_pinned_actions = start_pinned;
    let preview_id_actions = preview_id.clone();
    let emit_extension_events_actions = emit_extension_events;

    glib::idle_add_local_once(move || {
        let Some(window) = window_actions.upgrade() else {
            return;
        };

        let uri = match file_uri(&path_actions) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to enable drag/drop for preview: {e}");
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
        let drag_paintable = WidgetPaintable::new(Some(&card_actions));
        drag_source.set_icon(Some(&drag_paintable), 24, 24);

        let window_weak_drag = window.downgrade();
        let pinned_drag = pinned_actions.clone();
        let edit_opened_drag = edit_opened_actions.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            if edit_opened_drag.load(Ordering::Relaxed) {
                return;
            }
            if !should_dismiss_for_behavior(
                pinned_drag.load(Ordering::Relaxed),
                dismiss_after_dragging,
            ) {
                return;
            }
            if let Some(window) = window_weak_drag.upgrade() {
                dismiss_preview_window(&window, dismiss_action_actions);
            }
        });
        // Keep drag on the full card. Overlay buttons still win short clicks;
        // only a drag past the threshold starts DND.
        card_actions.add_controller(drag_source);

        let window_weak_close = window.downgrade();
        close_btn_actions.connect_clicked(move |_| {
            if let Some(window) = window_weak_close.upgrade() {
                window.close();
            }
        });

        if start_pinned_actions {
            pin_icon_actions.set_icon_name(Some(crate::capture::editor::window::icon_names::PIN));
            pin_btn_actions.add_css_class("preview-pinned");
            pin_btn_actions.set_tooltip_text(Some(&t("Unpin")));
        }
        let pin_state = pinned_actions.clone();
        let auto_close_anchor_pin = auto_close_anchor_actions.clone();
        let pin_icon_click = pin_icon_actions.clone();
        let pin_btn_click = pin_btn_actions.clone();
        let window_weak_pin = window.downgrade();
        let preview_id_pin = preview_id_actions.clone();
        let emit_pin = emit_extension_events_actions;
        pin_btn_actions.connect_clicked(move |_| {
            let now_pinned = !pin_state.load(Ordering::Relaxed);
            pin_state.store(now_pinned, Ordering::Relaxed);

            if !now_pinned {
                if let Ok(mut anchor) = auto_close_anchor_pin.lock() {
                    *anchor = Instant::now();
                }
            }

            if now_pinned {
                pin_icon_click.set_icon_name(Some(crate::capture::editor::window::icon_names::PIN));
                pin_btn_click.add_css_class("preview-pinned");
                pin_btn_click.set_tooltip_text(Some(&t("Unpin")));
            } else {
                pin_icon_click
                    .set_icon_name(Some(crate::capture::editor::window::icon_names::VIEW_PIN));
                pin_btn_click.remove_css_class("preview-pinned");
                pin_btn_click.set_tooltip_text(Some(&t("Pin")));
            }

            apply_preview_stacking(
                window_weak_pin.upgrade().as_ref(),
                &preview_id_pin,
                emit_pin,
                preview_should_stay_on_top(now_pinned),
            );
        });

        let path_copy = path_actions.clone();
        copy_btn_actions.connect_clicked(move |_| {
            if let Err(e) = copy_screenshot_to_clipboard(&path_copy) {
                eprintln!("Copy failed: {e}");
            }
        });

        let path_upload = path_actions.clone();
        // Worker thread signals the GTK main loop via this channel (GTK widgets
        // are !Send, so we cannot touch the window/button from the upload thread).
        let (upload_ui_tx, upload_ui_rx) = std::sync::mpsc::channel::<UploadUiEvent>();
        let uploading = Rc::new(Cell::new(false));
        let window_weak_upload_poll = window.downgrade();
        let dismiss_action_upload_poll = dismiss_action_actions;
        let upload_btn_poll = upload_btn_actions.clone();
        let uploading_poll = uploading.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            match upload_ui_rx.try_recv() {
                Ok(UploadUiEvent::Finished { dismiss, share_url }) => {
                    uploading_poll.set(false);
                    upload_btn_poll.set_sensitive(true);
                    if let Some(share_url) = share_url {
                        if let Err(error) =
                            crate::utils::clipboard::copy_text_to_gtk_clipboard(&share_url)
                        {
                            eprintln!("[preview] Failed to copy share link: {error}");
                        }
                    }
                    if dismiss {
                        if let Some(window) = window_weak_upload_poll.upgrade() {
                            dismiss_preview_window(&window, dismiss_action_upload_poll);
                        }
                    }
                    // Keep listening so a later upload (after a failure) can still finish/dismiss.
                    ControlFlow::Continue
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => ControlFlow::Break,
            }
        });
        let uploading_click = uploading.clone();
        let upload_btn_click = upload_btn_actions.clone();
        upload_btn_actions.connect_clicked(move |_| {
            // Ignore re-entrant clicks while an upload is already in flight.
            if uploading_click.get() {
                return;
            }

            let config = load_config();
            if !crate::cloud::upload::is_configured(&config) {
                let (title, body) = crate::cloud::upload::not_configured_notification(&config);
                crate::utils::notify::desktop_notification_important(&title, &body);
                return;
            }

            uploading_click.set(true);
            upload_btn_click.set_sensitive(false);

            // Live Settings value so Quick Access toggles apply without reopening.
            let close_after_upload = config.quick_access_close_after_uploading;
            let path = path_upload.clone();
            let upload_ui_tx = upload_ui_tx.clone();
            std::thread::spawn(move || {
                // Shared upload path (logs + notifications). Auto-upload after
                // capture uses the same helper so behavior stays consistent.
                let (dismiss, share_url) =
                    match crate::cloud::upload::upload_file_with_notifications_without_clipboard(
                        &config, &path,
                    ) {
                        Ok(result) => {
                            // Honor Quick Access "Close window after uploading".
                            // Failed uploads never dismiss the preview.
                            (
                                should_close_preview_after_upload(close_after_upload),
                                Some(result.share_url),
                            )
                        }
                        Err(_) => (false, None),
                    };
                let _ = upload_ui_tx.send(UploadUiEvent::Finished { dismiss, share_url });
            });
        });

        let path_edit = path_actions.clone();
        let source_bytes_edit = source_bytes_actions.clone();
        let edit_opened_btn = edit_opened_actions.clone();
        let window_weak_edit = window.downgrade();
        edit_btn_actions.connect_clicked(move |_| {
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

            if crate::preview_launch::should_use_direct_editor_launch() {
                if let Err(e) = crate::capture::open_image_editor(path_edit.clone()) {
                    eprintln!("Edit failed: {e}");
                    return;
                }
            } else {
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
                if let Err(e) = Command::new(&exe).arg("edit").arg(&path_edit).spawn() {
                    eprintln!("Edit failed: {e}");
                    return;
                }
            }

            edit_opened_btn.store(true, Ordering::Relaxed);

            if let Some(window) = window_weak_edit.upgrade() {
                window.close();
            }
        });

        if probe.enabled {
            eprintln!(
                "[preview] deferred actions initialized at {}ms for {}",
                startup_actions.elapsed().as_millis(),
                path_actions.display()
            );
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

    let app_close = app.clone();
    let edit_opened_close = edit_opened.clone();
    let preview_id_close = preview_id.clone();
    window.connect_close_request(move |_| {
        if !edit_opened_close.load(Ordering::Relaxed) {
            app_close.quit();
        }
        if emit_extension_events {
            crate::gnome_integration::emit_tracked_window_closed(&preview_id_close);
        }
        glib::Propagation::Proceed
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

    // Pin owns always-on-top. Unpinned previews stay in the corner but other
    // windows can cover them. Editor / capture overlay use their own tracked IDs.
    if preview_should_stay_on_top(start_pinned) {
        apply_preview_stacking(Some(&window), &preview_id, emit_extension_events, true);
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

            #capture-preview-chrome {
                background-color: transparent;
                background-image: none;
                box-shadow: none;
                padding: 10px;
            }

            #capture-preview-card {
                background-color: #141414;
                border-radius: 16px;
                border: 1px solid rgba(255, 255, 255, 0.10);
                box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
                padding: 0;
                outline-width: 0;
            }

            #capture-preview-image-frame {
                border-radius: 12px;
                border: 1px solid rgba(255, 255, 255, 0.04);
            }

            #capture-preview-image {
                border-radius: 0;
            }

            button.preview-corner-btn {
                min-width: 30px;
                min-height: 30px;
                padding: 0;
                border-radius: 10px;
                border: 1px solid rgba(255, 255, 255, 0.12);
                background: rgba(18, 18, 18, 0.92);
                background-image: none;
                color: rgba(255, 255, 255, 0.92);
                box-shadow: none;
                outline-width: 0;
                transition: background 120ms ease, color 120ms ease, border-color 120ms ease;
            }

            button.preview-corner-btn:hover {
                background: rgba(42, 42, 42, 0.96);
                background-image: none;
                color: #ffffff;
            }

            button.preview-corner-btn:active {
                background: rgba(12, 12, 12, 0.96);
                background-image: none;
                color: rgba(255, 255, 255, 0.88);
            }

            button.preview-corner-btn:focus,
            button.preview-corner-btn:focus-visible {
                outline: none;
                box-shadow: none;
            }

            button.preview-corner-btn.preview-pinned {
                background: rgba(255, 255, 255, 0.16);
                background-image: none;
            }

            button.preview-close-btn:hover,
            button.preview-close-btn:hover:focus {
                background: #e81123;
                background-image: none;
                border-color: transparent;
                color: #ffffff;
            }

            button.preview-close-btn:active {
                background: #c50f1f;
                background-image: none;
                color: #ffffff;
            }

            button.preview-copy-btn {
                min-width: 72px;
                min-height: 30px;
                padding: 0 16px;
                border-radius: 999px;
                border: none;
                background: #f3f4f6;
                background-image: none;
                color: #111111;
                font-family: 'Inter', 'Noto Sans', system-ui, sans-serif;
                font-size: 13px;
                font-weight: 600;
                letter-spacing: 0.01em;
                box-shadow: none;
                outline-width: 0;
                transition: background 120ms ease, color 120ms ease;
            }

            button.preview-copy-btn:hover {
                background: #ffffff;
                background-image: none;
                color: #111111;
            }

            button.preview-copy-btn:active {
                background: #e5e7eb;
                background-image: none;
                color: #111111;
            }

            button.preview-copy-btn:focus,
            button.preview-copy-btn:focus-visible {
                outline: none;
                box-shadow: none;
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

fn icon_button(icon_name: &str, tooltip: &str) -> (Button, gtk4::Image) {
    let image = gtk4::Image::from_icon_name(icon_name);
    image.set_pixel_size(16);

    let button = Button::new();
    button.set_child(Some(&image));
    button.set_tooltip_text(Some(tooltip));
    button.set_has_frame(false);
    button.set_focusable(false);

    (button, image)
}

fn corner_icon_button(
    icon_name: &str,
    tooltip: &str,
    halign: Align,
    valign: Align,
) -> (Button, gtk4::Image) {
    let (button, image) = icon_button(icon_name, tooltip);
    button.add_css_class("preview-corner-btn");
    button.set_halign(halign);
    button.set_valign(valign);
    if halign == Align::Start {
        button.set_margin_start(PREVIEW_CORNER_MARGIN);
    } else if halign == Align::End {
        button.set_margin_end(PREVIEW_CORNER_MARGIN);
    }
    if valign == Align::Start {
        button.set_margin_top(PREVIEW_CORNER_MARGIN);
    } else if valign == Align::End {
        button.set_margin_bottom(PREVIEW_CORNER_MARGIN);
    }
    (button, image)
}

fn copy_pill_button(label: &str) -> Button {
    let button = Button::with_label(label);
    button.set_tooltip_text(Some(label));
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_halign(Align::Center);
    button.set_valign(Align::Center);
    button.add_css_class("preview-copy-btn");
    button
}

fn file_uri(path: &Path) -> Result<String, CapturePreviewError> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| CapturePreviewError::InvalidPath)
}

fn copy_screenshot_to_clipboard(path: &Path) -> Result<(), CapturePreviewError> {
    crate::utils::clipboard::copy_image_to_clipboard(path).map_err(|e| {
        if e.contains("not found") {
            CapturePreviewError::ClipboardToolNotFound
        } else {
            CapturePreviewError::ClipboardCommandFailed
        }
    })
}

/// Compute the card's input region in surface-local coordinates.
///
/// `Widget::allocation()` is parent-relative and can miss window chrome / overlay
/// offsets. On KDE Wayland that produced a region that did not cover the visible
/// card, so pointer events passed through (no drag, no toolbar clicks).
fn card_input_region_rect(window: &Window, card: &Overlay) -> Option<(i32, i32, i32, i32)> {
    let bounds = card.compute_bounds(window)?;
    let x = bounds.x().floor() as i32;
    let y = bounds.y().floor() as i32;
    let width = bounds.width().ceil() as i32;
    let height = bounds.height().ceil() as i32;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((x, y, width, height))
}

/// Restrict pointer hit-testing to the preview card on full-surface fallback windows.
/// Returns `true` when the region was applied to a live surface.
fn apply_fallback_input_region(window: &Window, card: &Overlay) -> bool {
    let Some(surface) = window.surface() else {
        return false;
    };
    let Some((x, y, width, height)) = card_input_region_rect(window, card) else {
        return false;
    };

    let region_rect = gtk4::cairo::RectangleInt::new(x, y, width, height);
    let input_region = gtk4::cairo::Region::create_rectangle(&region_rect);
    surface.set_input_region(&input_region);
    true
}

fn install_fallback_input_region_tracking(window: &Window, card: &Overlay) {
    let window_weak = window.downgrade();
    let card_weak = card.downgrade();
    let last_region = Rc::new(RefCell::new(None::<(i32, i32, i32, i32)>));

    let reapply_region: Rc<dyn Fn()> = Rc::new({
        let window_weak = window_weak.clone();
        let card_weak = card_weak.clone();
        let last_region = last_region.clone();
        move || {
            let (Some(window), Some(card)) = (window_weak.upgrade(), card_weak.upgrade()) else {
                return;
            };

            let Some(next_region) = card_input_region_rect(&window, &card) else {
                return;
            };

            if last_region.borrow().as_ref() == Some(&next_region) {
                return;
            }

            // Only cache the region after a successful apply. Caching before the
            // GdkSurface exists would permanently skip reapplication.
            if apply_fallback_input_region(&window, &card) {
                *last_region.borrow_mut() = Some(next_region);
            }
        }
    });

    window.connect_map({
        let reapply_region = reapply_region.clone();
        move |_| {
            let reapply_region = reapply_region.clone();
            glib::idle_add_local_once(move || reapply_region());
        }
    });

    card.add_tick_callback({
        let reapply_region = reapply_region.clone();
        move |_, _| {
            reapply_region();
            ControlFlow::Continue
        }
    });

    glib::timeout_add_local(Duration::from_millis(250), move || {
        if window_weak.upgrade().is_none() || card_weak.upgrade().is_none() {
            return ControlFlow::Break;
        }

        reapply_region();
        ControlFlow::Continue
    });
}

fn is_non_x11_surface_error(err: &str) -> bool {
    err.contains("surface is not X11")
}

fn request_x11_always_on_top(window: &Window, enabled: bool) -> Result<(), String> {
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
    let action = if enabled { 1 } else { 0 };

    send_net_wm_state_client_message(
        &conn,
        root,
        xid,
        net_wm_state,
        action,
        net_wm_state_above,
        0,
    )?;
    send_net_wm_state_client_message(
        &conn,
        root,
        xid,
        net_wm_state,
        action,
        net_wm_state_sticky,
        0,
    )?;

    if enabled {
        conn.configure_window(
            xid,
            &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
        )
        .map_err(|e| e.to_string())?;
    }

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

fn build_preview_area(
    path: PathBuf,
    preview_width: i32,
    preview_height: i32,
    startup: Instant,
    probe: PreviewStartupProbe,
) -> DrawingArea {
    probe.log("build-preview-area-enter");
    let area = DrawingArea::new();
    probe.log("after-drawing-area-new");
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

        // Use max to ensure the image covers the area (cropping the excess)
        let scale = (width as f64 / tw).max(height as f64 / th);
        let sw = tw * scale;
        let sh = th * scale;
        let ox = (width as f64 - sw) / 2.0;
        let oy = (height as f64 - sh) / 2.0;

        let snapshot = gtk4::Snapshot::new();
        // Clip to drawing area bounds to hide cropped overflow
        snapshot.push_clip(&gtk4::graphene::Rect::new(
            0.0,
            0.0,
            width as f32,
            height as f32,
        ));
        snapshot.translate(&gtk4::graphene::Point::new(ox as f32, oy as f32));
        tex.snapshot(&snapshot, sw, sh);
        snapshot.pop();
        if let Some(node) = snapshot.to_node() {
            node.draw(cr);
        }
    });

    // Decode the screenshot on a background thread so the preview card can
    // appear immediately without blocking the GTK main loop on PNG decode.
    let (tx, rx) = std::sync::mpsc::channel::<Option<(Vec<u8>, i32, i32)>>();
    let path_decode = path.clone();
    std::thread::spawn(move || {
        let decoded = image::open(&path_decode)
            .map(|img| {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                (rgba.into_raw(), w as i32, h as i32)
            })
            .ok();
        if decoded.is_none() {
            eprintln!(
                "Preview thumbnail warning: failed to decode screenshot at {}",
                path_decode.display()
            );
        }
        let _ = tx.send(decoded);
    });

    let area_weak = area.downgrade();
    let texture_main = texture.clone();
    let path_fallback = path.clone();
    let probe_async = probe.clone();
    glib::timeout_add_local(Duration::from_millis(4), move || {
        match rx.try_recv() {
            Ok(Some((data, width, height))) => {
                let stride = width.saturating_mul(4);
                let bytes = gtk4::glib::Bytes::from_owned(data);
                let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_bytes(
                    &bytes,
                    gtk4::gdk_pixbuf::Colorspace::Rgb,
                    true,
                    8,
                    width,
                    height,
                    stride,
                );
                *texture_main.borrow_mut() = Some(gdk::Texture::for_pixbuf(&pixbuf));
                if let Some(area) = area_weak.upgrade() {
                    area.queue_draw();
                }
                probe_async.log("async-texture-ready");
                if probe_async.enabled {
                    eprintln!(
                        "[preview] async texture ready at {}ms for {}",
                        startup.elapsed().as_millis(),
                        path_fallback.display()
                    );
                }
                ControlFlow::Break
            }
            Ok(None) => {
                // Background decode failed – fall back to the synchronous
                // gdk-pixbuf loader on the main thread so the preview is
                // never empty if the worker thread choked.
                *texture_main.borrow_mut() = preview_texture(&path_fallback);
                if let Some(area) = area_weak.upgrade() {
                    area.queue_draw();
                }
                probe_async.log("fallback-texture-ready");
                if probe_async.enabled {
                    eprintln!(
                        "[preview] fallback texture ready at {}ms for {}",
                        startup.elapsed().as_millis(),
                        path_fallback.display()
                    );
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => ControlFlow::Break,
        }
    });

    area
}

fn preview_texture(path: &Path) -> Option<gdk::Texture> {
    // Synchronous fallback used only if the background decoder thread fails.
    let preview_pixbuf = match gtk4::gdk_pixbuf::Pixbuf::from_file(path) {
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

fn configure_window_positioning(window: &Window, side: PreviewSide, _preview_width: i32) -> bool {
    if std::env::var_os(PREVIEW_DISABLE_LAYER_SHELL_ENV).is_some() {
        return false;
    }

    // GNOME Shell does not expose layer-shell surfaces as MetaWindows, so the
    // helper extension cannot `make_above()` them. Stay on the regular-window
    // fallback and let `TrackedWindowOpened` keep the preview on top.
    if is_gnome_wayland_session() {
        return false;
    }

    // Use the same layer-shell setup on compositors that support it
    // (KDE Plasma 6+, wlroots, …). Ubuntu/GNOME stays on the extension path
    // above. The Fedora bug was a KDE-only early-return that forced a
    // full-screen fallback + input-region instead; that return is
    // intentionally gone so KDE joins this proven path.
    if gtk4_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_namespace(Some("apexshot-capture-preview"));
        window.set_layer(Layer::Overlay);

        window.set_anchor(Edge::Left, side == PreviewSide::Left);
        window.set_anchor(Edge::Right, side == PreviewSide::Right);
        window.set_anchor(Edge::Top, false);
        window.set_anchor(Edge::Bottom, true);

        // The screenshot preview is a transient floating card, not a panel.
        // Do not reserve compositor-managed edge space for it, otherwise some
        // desktops may treat the reserved strip as owned by the preview and
        // block clicks on windows behind it.
        window.set_exclusive_zone(0);
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
    fn preview_extension_signals_are_emitted_without_layer_shell() {
        assert!(should_emit_extension_events(false));
        assert!(!should_emit_extension_events(true));
    }

    #[test]
    fn preview_skips_layer_shell_on_gnome_wayland() {
        assert!(is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            Some("ubuntu:GNOME"),
            Some("ubuntu"),
        ));
        assert!(is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            Some("GNOME"),
            Some("gnome"),
        ));
        assert!(!is_gnome_wayland_session_from_env(
            Some("wayland-0"),
            Some("KDE"),
            Some("plasma"),
        ));
        assert!(!is_gnome_wayland_session_from_env(
            None,
            Some("ubuntu:GNOME"),
            Some("ubuntu"),
        ));
    }

    #[test]
    fn preview_starts_pinned_when_auto_close_is_disabled() {
        assert!(initial_preview_pinned(false));
        assert!(!initial_preview_pinned(true));
    }

    #[test]
    fn close_after_upload_follows_checkbox_literally() {
        // Upload dismiss is gated only on the setting (not pin state).
        assert!(should_close_preview_after_upload(true));
        assert!(!should_close_preview_after_upload(false));
    }

    #[test]
    fn preview_overlay_uses_framed_card_with_corner_actions_and_copy_pill() {
        let source = include_str!("preview_overlay.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("preview-corner-btn")
                && production.contains("preview-copy-btn")
                && production.contains("preview-close-btn")
                && production.contains("corner_icon_button")
                && production.contains("copy_pill_button")
                && production.contains("ARROW_EXPORT_UP_REGULAR")
                && production.contains("copy_screenshot_to_clipboard")
                && !production.contains("preview-tools")
                && !production.contains("preview-close-label"),
            "quick-access overlay must be a framed screenshot with corner actions and a Copy pill"
        );
    }

    #[test]
    fn preview_chrome_includes_frame_inset_and_shadow_pad() {
        assert_eq!(
            preview_chrome_padding(),
            PREVIEW_FRAME_INSET * 2 + PREVIEW_SHADOW_PAD * 2
        );
    }

    #[test]
    fn preview_gnome_path_skips_layer_shell_and_announces_the_window() {
        let source = include_str!("preview_overlay.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("if is_gnome_wayland_session()")
                && production.contains("return false;")
                && production.contains("emit_tracked_window_opened")
                && production.contains("should_emit_extension_events(layer_shell_active)"),
            "GNOME preview must skip layer-shell and announce the window to the helper extension"
        );
    }

    #[test]
    fn pin_owns_always_on_top_and_unpin_releases_it() {
        assert!(preview_should_stay_on_top(true));
        assert!(!preview_should_stay_on_top(false));

        let source = include_str!("preview_overlay.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("apply_preview_stacking")
                && production.contains("emit_tracked_window_closed(preview_id)")
                && production.contains("request_x11_always_on_top(window, stay_on_top)")
                && production.contains("preview_should_stay_on_top(now_pinned)")
                && production.contains("preview_should_stay_on_top(pinned_watchdog.load"),
            "pin must raise the preview; unpin must drop always-on-top without touching other tracked windows"
        );
    }

    #[test]
    fn preview_button_css_has_no_drop_shadow() {
        let source = include_str!("preview_overlay.rs");
        let css = source
            .split("button.preview-corner-btn {")
            .nth(1)
            .and_then(|rest| rest.split("button.preview-copy-btn:focus-visible").next())
            .unwrap_or("");
        assert!(
            css.contains("box-shadow: none") && !css.contains("box-shadow: 0"),
            "corner and copy buttons must not paint a drop shadow"
        );
    }
}
