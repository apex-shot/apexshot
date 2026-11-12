use super::windowing::prefers_dark_glass_theme;
use crate::i18n::{empty_shortcut_label, t};
use crate::{config::AppConfig, daemon::set_daemon_hotkey_suppressed};
use gtk4::{
    gdk, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, EventControllerKey, Image,
    Label, Orientation,
};
#[cfg(not(target_os = "windows"))]
use gtk4::{Dialog, ResponseType};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

fn normalize_shortcut_key_name(key_name: &str) -> Option<String> {
    let trimmed = key_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "control_l"
            | "control_r"
            | "shift_l"
            | "shift_r"
            | "alt_l"
            | "alt_r"
            | "super_l"
            | "super_r"
            | "meta_l"
            | "meta_r"
    ) {
        return None;
    }

    let normalized = match lower.as_str() {
        "escape" => "Esc".to_string(),
        "backspace" => "BackSpace".to_string(),
        "page_up" => "PageUp".to_string(),
        "page_down" => "PageDown".to_string(),
        "print" => "Print".to_string(),
        _ if trimmed.len() == 1 => trimmed.to_ascii_uppercase(),
        _ => {
            let mut chars = lower.chars();
            let first = chars.next()?;
            first.to_ascii_uppercase().to_string() + chars.as_str()
        }
    };

    Some(normalized)
}

fn compose_shortcut_parts(
    key_name: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> Option<Vec<String>> {
    let key = normalize_shortcut_key_name(key_name)?;
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if super_key {
        parts.push("Super".to_string());
    }
    parts.push(key);
    Some(parts)
}

fn compose_shortcut_label(
    key_name: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> Option<String> {
    Some(compose_shortcut_parts(key_name, ctrl, alt, shift, super_key)?.join("+"))
}

#[cfg(target_os = "windows")]
fn unmodified_shortcut_key(
    display: &gdk::Display,
    keycode: u32,
    layout: i32,
    fallback: gdk::Key,
) -> gdk::Key {
    // Ctrl+Alt can act as AltGr, so the translated character may not be the
    // key Windows needs for RegisterHotKey.
    display
        .translate_key(keycode, gdk::ModifierType::empty(), layout)
        .map(|(key, _, _, _)| key)
        .filter(|key| key.name().is_some())
        .unwrap_or(fallback)
}

#[derive(Debug, PartialEq, Eq)]
enum ShortcutKeyOutcome {
    Ignore,
    Disable,
    Capture { parts: Vec<String>, label: String },
}

fn handle_shortcut_key(
    suppression_ready: bool,
    key_name: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> ShortcutKeyOutcome {
    if !suppression_ready || key_name == "Escape" {
        return ShortcutKeyOutcome::Ignore;
    }
    if key_name == "BackSpace" && !ctrl && !alt && !shift && !super_key {
        return ShortcutKeyOutcome::Disable;
    }
    match (
        compose_shortcut_parts(key_name, ctrl, alt, shift, super_key),
        compose_shortcut_label(key_name, ctrl, alt, shift, super_key),
    ) {
        (Some(parts), Some(label)) => ShortcutKeyOutcome::Capture { parts, label },
        _ => ShortcutKeyOutcome::Ignore,
    }
}

#[cfg(target_os = "windows")]
fn request_hotkey_suppressed(suppressed: bool) -> bool {
    set_daemon_hotkey_suppressed(suppressed).is_ok()
}

#[cfg(not(target_os = "windows"))]
fn request_hotkey_suppressed(suppressed: bool) -> bool {
    std::thread::spawn(move || {
        let _ = set_daemon_hotkey_suppressed(suppressed);
    });
    true
}

fn captured_keycaps_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("shortcut-capture-keycaps-row");
    #[cfg(target_os = "windows")]
    {
        row.set_halign(Align::Center);
        row.set_valign(Align::Center);
        row.set_vexpand(true);
    }
    row
}

/// The Set Shortcut panel: a header with Cancel, the title and Set, and a body
/// that steps from arming to listening to the captured keys.
struct CapturePanel {
    header: GtkBox,
    body: GtkBox,
    cancel_btn: Button,
    set_btn: Button,
    stack: gtk4::Stack,
    arming_label: Label,
    keycaps_row: GtkBox,
}

fn build_capture_panel(action_name: &str) -> CapturePanel {
    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.set_margin_top(10);
    header.set_margin_bottom(10);
    header.set_margin_start(14);
    header.set_margin_end(14);

    let cancel_btn = Button::with_label(&t("Cancel"));
    cancel_btn.add_css_class("shortcut-capture-secondary-btn");
    cancel_btn.set_valign(Align::Center);
    let title = Label::new(Some(&t("Set Shortcut")));
    title.add_css_class("shortcut-capture-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_valign(Align::Center);
    let set_btn = Button::with_label(&t("Set"));
    set_btn.add_css_class("shortcut-capture-primary-btn");
    set_btn.set_valign(Align::Center);
    set_btn.set_sensitive(false);

    header.append(&cancel_btn);
    header.append(&title);
    header.append(&set_btn);

    let body = GtkBox::new(Orientation::Vertical, 16);
    #[cfg(target_os = "windows")]
    body.set_vexpand(true);
    body.set_margin_top(12);
    body.set_margin_bottom(20);
    body.set_margin_start(28);
    body.set_margin_end(28);

    let subtitle = Label::new(Some(&format!(
        "Enter new shortcut to change {}",
        action_name
    )));
    subtitle.set_wrap(true);
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("shortcut-capture-subtitle");
    body.append(&subtitle);

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    #[cfg(target_os = "windows")]
    stack.set_vexpand(true);

    let arming_box = GtkBox::new(Orientation::Vertical, 14);
    arming_box.set_vexpand(true);
    let arming_label = Label::new(Some(&t("Preparing shortcut capture…")));
    arming_label.add_css_class("shortcut-capture-hint");
    arming_label.set_xalign(0.0);
    arming_box.append(&arming_label);
    stack.add_named(&arming_box, Some("arming"));

    let listening_box = GtkBox::new(Orientation::Vertical, 14);
    listening_box.set_vexpand(true);
    let listening_icon = Label::new(Some(&t("⌄")));
    listening_icon.add_css_class("shortcut-capture-listening-icon");
    listening_box.append(&listening_icon);
    let listening_hint = Label::new(Some(&t(
        "Press Esc to cancel or Backspace to disable the keyboard shortcut",
    )));
    listening_hint.set_wrap(true);
    listening_hint.set_xalign(0.0);
    listening_hint.add_css_class("shortcut-capture-hint");
    listening_box.append(&listening_hint);
    stack.add_named(&listening_box, Some("listening"));

    let captured_box = GtkBox::new(Orientation::Vertical, 18);
    captured_box.set_vexpand(true);
    let keycaps_row = captured_keycaps_row();
    captured_box.append(&keycaps_row);
    stack.add_named(&captured_box, Some("captured"));
    stack.set_visible_child_name("arming");
    body.append(&stack);

    CapturePanel {
        header,
        body,
        cancel_btn,
        set_btn,
        stack,
        arming_label,
        keycaps_row,
    }
}

/// Keys pressed while the panel listens. Esc runs `cancel`; Backspace alone
/// marks the shortcut for removal; any other key with its modifiers is shown
/// as keycaps and enables Set.
fn capture_key_controller(
    panel: &CapturePanel,
    captured: Rc<RefCell<Option<String>>>,
    suppression_ready: Rc<Cell<bool>>,
    cancel: impl Fn() + 'static,
) -> EventControllerKey {
    let keycaps_for_keys = panel.keycaps_row.clone();
    let stack_for_keys = panel.stack.clone();
    let set_btn_for_keys = panel.set_btn.clone();

    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    key_controller.connect_key_pressed(move |controller, key, keycode, state| {
        if key == gdk::Key::Escape {
            cancel();
            return gtk4::glib::Propagation::Stop;
        }
        let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
        let alt = state.contains(gdk::ModifierType::ALT_MASK);
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let super_key = state.contains(gdk::ModifierType::SUPER_MASK)
            || state.contains(gdk::ModifierType::META_MASK);

        #[cfg(target_os = "windows")]
        let key = controller
            .current_event()
            .and_then(|event| event.downcast::<gdk::KeyEvent>().ok())
            .and_then(|event| {
                controller.widget().map(|widget| {
                    unmodified_shortcut_key(&widget.display(), keycode, event.layout() as i32, key)
                })
            })
            .unwrap_or(key);
        #[cfg(not(target_os = "windows"))]
        let _ = (controller, keycode);

        if let Some(name) = key.name() {
            match handle_shortcut_key(suppression_ready.get(), &name, ctrl, alt, shift, super_key) {
                ShortcutKeyOutcome::Ignore => return gtk4::glib::Propagation::Proceed,
                ShortcutKeyOutcome::Disable => {
                    captured.replace(Some(String::new()));
                    while let Some(child) = keycaps_for_keys.first_child() {
                        keycaps_for_keys.remove(&child);
                    }
                    let cleared = Label::new(Some(&t("Shortcut will be disabled")));
                    cleared.add_css_class("shortcut-capture-cleared-label");
                    keycaps_for_keys.append(&cleared);
                    stack_for_keys.set_visible_child_name("captured");
                    set_btn_for_keys.set_sensitive(true);
                    return gtk4::glib::Propagation::Stop;
                }
                ShortcutKeyOutcome::Capture { parts, label } => {
                    captured.replace(Some(label));
                    while let Some(child) = keycaps_for_keys.first_child() {
                        keycaps_for_keys.remove(&child);
                    }

                    for (idx, part) in parts.iter().enumerate() {
                        let keycap = Label::new(Some(part));
                        keycap.add_css_class("shortcut-capture-keycap");
                        keycaps_for_keys.append(&keycap);
                        if idx + 1 != parts.len() {
                            let plus = Label::new(Some(&t("+")));
                            plus.add_css_class("shortcut-capture-plus");
                            keycaps_for_keys.append(&plus);
                        }
                    }

                    stack_for_keys.set_visible_child_name("captured");
                    set_btn_for_keys.set_sensitive(true);
                    return gtk4::glib::Propagation::Stop;
                }
            }
        }

        gtk4::glib::Propagation::Proceed
    });
    key_controller
}

/// Pause the global hotkeys so the keys reach the panel, then start listening.
fn arm_capture(panel: &CapturePanel, suppression_ready: &Cell<bool>) {
    if request_hotkey_suppressed(true) {
        panel.stack.set_visible_child_name("listening");
        suppression_ready.set(true);
    } else {
        panel.arming_label.set_text(&t(
            "Could not pause the shortcut listener. Close this dialog and try again.",
        ));
    }
}

fn apply_captured_shortcut(button: &Button, captured: &RefCell<Option<String>>) {
    if let Some(text) = captured.borrow().clone() {
        if text.is_empty() {
            button.set_label(&empty_shortcut_label());
        } else {
            button.set_label(&text);
        }
    }
}

fn install_shortcut_editor(button: &Button, parent: &ApplicationWindow, editing: &Rc<Cell<bool>>) {
    let button = button.clone();
    let parent = parent.clone();
    let editing = editing.clone();
    button.clone().connect_clicked(move |_| {
        if editing.replace(true) {
            return;
        }
        let action_name = button.label().unwrap_or_else(|| "shortcut".into());
        #[cfg(target_os = "windows")]
        present_capture_sheet(&parent, &button, &editing, &action_name);
        #[cfg(not(target_os = "windows"))]
        present_capture_dialog(&parent, &button, &editing, &action_name);
    });
}

#[cfg(not(target_os = "windows"))]
fn present_capture_dialog(
    parent: &ApplicationWindow,
    button: &Button,
    editing: &Rc<Cell<bool>>,
    action_name: &str,
) {
    let panel = build_capture_panel(action_name);
    let dialog = Dialog::builder()
        .transient_for(parent)
        .modal(true)
        .title(t("Set Shortcut"))
        .deletable(false)
        .default_width(400)
        .default_height(290)
        .build();
    dialog.add_css_class("shortcut-capture-dialog");
    if !prefers_dark_glass_theme() {
        dialog.add_css_class("editor-theme-light");
    }
    dialog.set_titlebar(Some(&panel.header));
    let content = dialog.content_area();
    content.set_spacing(0);
    content.append(&panel.body);

    let captured = Rc::new(RefCell::new(None::<String>));
    let suppression_ready = Rc::new(Cell::new(false));
    let dialog_for_keys = dialog.downgrade();
    dialog.add_controller(capture_key_controller(
        &panel,
        captured.clone(),
        suppression_ready.clone(),
        move || {
            if let Some(dialog) = dialog_for_keys.upgrade() {
                dialog.response(ResponseType::Cancel);
            }
        },
    ));

    let dialog_for_cancel = dialog.clone();
    panel
        .cancel_btn
        .connect_clicked(move |_| dialog_for_cancel.response(ResponseType::Cancel));
    let dialog_for_set = dialog.clone();
    panel
        .set_btn
        .connect_clicked(move |_| dialog_for_set.response(ResponseType::Accept));

    let button_for_response = button.clone();
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            apply_captured_shortcut(&button_for_response, &captured);
        }
        let _ = request_hotkey_suppressed(false);
        dialog.close();
    });

    let editing_for_close = editing.clone();
    dialog.connect_close_request(move |_| {
        editing_for_close.set(false);
        let _ = request_hotkey_suppressed(false);
        gtk4::glib::Propagation::Proceed
    });

    dialog.present();
    arm_capture(&panel, &suppression_ready);
}

/// On Windows the panel is a sheet inside Settings rather than a window of its
/// own. A scrim covers the window, so nothing under it can change while a
/// shortcut is recorded; a click on the scrim cancels, as does Esc.
#[cfg(target_os = "windows")]
fn present_capture_sheet(
    parent: &ApplicationWindow,
    button: &Button,
    editing: &Rc<Cell<bool>>,
    action_name: &str,
) {
    // Settings sets an overlay as the window's child; the save toast rides on
    // it too.
    let Some(host) = parent.child().and_downcast::<gtk4::Overlay>() else {
        editing.set(false);
        return;
    };
    let panel = build_capture_panel(action_name);

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("shortcut-capture-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("shortcut-capture-dialog");
    card.add_css_class("shortcut-capture-windows");
    card.add_css_class("shortcut-capture-sheet");
    if !prefers_dark_glass_theme() {
        card.add_css_class("editor-theme-light");
    }
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_size_request(400, 290);
    card.append(&panel.header);
    card.append(&panel.body);

    host.add_overlay(&scrim);
    host.add_overlay(&card);

    let captured = Rc::new(RefCell::new(None::<String>));
    let suppression_ready = Rc::new(Cell::new(false));
    let keys_slot: Rc<RefCell<Option<gtk4::glib::WeakRef<EventControllerKey>>>> =
        Rc::new(RefCell::new(None));
    let close_slot: Rc<RefCell<Option<gtk4::glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));

    let finish: Rc<dyn Fn(bool)> = Rc::new({
        let host = host.downgrade();
        let scrim = scrim.downgrade();
        let card = card.downgrade();
        let parent = parent.downgrade();
        let button = button.clone();
        let editing = editing.clone();
        let captured = captured.clone();
        let keys_slot = keys_slot.clone();
        let close_slot = close_slot.clone();
        move |accept| {
            // Esc, the scrim, both buttons and closing Settings all end here;
            // only the first of them tears the sheet down.
            if !editing.replace(false) {
                return;
            }
            if accept {
                apply_captured_shortcut(&button, &captured);
            }
            let _ = request_hotkey_suppressed(false);
            if let Some(parent) = parent.upgrade() {
                if let Some(keys) = keys_slot
                    .borrow_mut()
                    .take()
                    .and_then(|keys| keys.upgrade())
                {
                    parent.remove_controller(&keys);
                }
                if let Some(id) = close_slot.borrow_mut().take() {
                    parent.disconnect(id);
                }
            }
            if let (Some(host), Some(scrim), Some(card)) =
                (host.upgrade(), scrim.upgrade(), card.upgrade())
            {
                host.remove_overlay(&card);
                host.remove_overlay(&scrim);
            }
        }
    });

    // Esc and the scrim click finish from an idle callback, so the controller
    // and the scrim are not removed while their own handlers still run.
    let finish_later = {
        let finish = finish.clone();
        move || {
            let finish = finish.clone();
            gtk4::glib::idle_add_local_once(move || finish(false));
        }
    };

    let keys = capture_key_controller(
        &panel,
        captured.clone(),
        suppression_ready.clone(),
        finish_later.clone(),
    );
    parent.add_controller(keys.clone());
    keys_slot.replace(Some(keys.downgrade()));

    let close_id = parent.connect_close_request({
        let finish = finish.clone();
        move |_| {
            finish(false);
            gtk4::glib::Propagation::Proceed
        }
    });
    close_slot.replace(Some(close_id));

    let dismiss = gtk4::GestureClick::new();
    dismiss.connect_released(move |_, _, _, _| finish_later());
    scrim.add_controller(dismiss);

    panel.cancel_btn.connect_clicked({
        let finish = finish.clone();
        move |_| finish(false)
    });
    panel.set_btn.connect_clicked({
        let finish = finish.clone();
        move |_| finish(true)
    });

    panel.cancel_btn.grab_focus();
    arm_capture(&panel, &suppression_ready);
}

pub fn install_shortcut_editors(widgets: &ShortcutSettingsWidgets, parent: &ApplicationWindow) {
    let editing = Rc::new(Cell::new(false));
    for button in [
        &widgets.open_file_btn,
        &widgets.open_clipboard_btn,
        &widgets.restore_file_btn,
        &widgets.toggle_overlays_btn,
        &widgets.quick_capture_btn,
        &widgets.capture_area_btn,
        &widgets.capture_crosshair_btn,
        &widgets.capture_prev_btn,
        &widgets.capture_fullscreen_btn,
        &widgets.capture_window_btn,
        &widgets.show_last_preview_btn,
        &widgets.save_last_capture_btn,
        &widgets.record_screen_btn,
        &widgets.recording_stop_save_btn,
        &widgets.recording_pause_btn,
        &widgets.recording_restart_btn,
        &widgets.recording_marker_btn,
    ] {
        install_shortcut_editor(button, parent, &editing);
    }
}

pub struct ShortcutSettingsWidgets {
    pub section: GtkBox,
    pub status_labels: HashMap<&'static str, Label>,
    pub open_file_btn: Button,
    pub open_clipboard_btn: Button,
    pub restore_file_btn: Button,
    pub toggle_overlays_btn: Button,
    pub quick_capture_btn: Button,
    pub capture_area_btn: Button,
    pub capture_crosshair_btn: Button,
    pub capture_prev_btn: Button,
    pub capture_fullscreen_btn: Button,
    pub capture_window_btn: Button,
    pub show_last_preview_btn: Button,
    pub save_last_capture_btn: Button,
    pub record_screen_btn: Button,
    pub recording_stop_save_btn: Button,
    /// Windows only, like the three rows that hold them.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub recording_pause_btn: Button,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub recording_restart_btn: Button,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub recording_marker_btn: Button,
}

pub fn build_shortcuts_section(config: &AppConfig) -> ShortcutSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let tip_text = if cfg!(target_os = "windows") {
        "Use shortcuts while ApexShot is running in the background."
    } else {
        "Shortcuts set here are the same hotkeys ApexShot uses. If one does not work, your desktop environment may already be using it. Open your system keyboard settings and disable conflicting shortcuts if you want to reuse them in ApexShot."
    };
    let tip = Label::new(Some(&t(tip_text)));
    tip.set_wrap(true);
    tip.set_xalign(0.0);
    tip.add_css_class("settings-sub-option-hint");
    tip.add_css_class("shortcuts-tip");
    tip.set_margin_bottom(20);
    section.append(&tip);

    macro_rules! build_row {
        ($content:expr, $is_muted:expr) => {{
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            row.add_css_class("settings-table-row");
            if $is_muted {
                row.add_css_class("settings-table-row-muted");
            }
            row.set_hexpand(true);
            row.append($content);
            row
        }};
    }

    let build_frame = || -> gtk4::Box {
        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("settings-table-frame");
        frame.set_margin_bottom(24);
        frame.set_margin_start(4);
        frame.set_margin_end(4);
        frame
    };

    let create_header = |section: &GtkBox, label_text: &str, icon: &str| {
        let hbox = GtkBox::new(Orientation::Horizontal, 10);
        #[cfg(target_os = "windows")]
        hbox.add_css_class("shortcut-heading-windows");
        hbox.set_margin_bottom(8);
        hbox.set_margin_top(8);

        let img = Image::from_icon_name(icon);
        img.set_pixel_size(20);
        #[cfg(target_os = "windows")]
        img.set_valign(Align::Center);

        let lbl = Label::new(Some(&t(label_text)));
        lbl.add_css_class("settings-group-title");
        #[cfg(target_os = "windows")]
        lbl.set_valign(Align::Center);

        hbox.append(&img);
        hbox.append(&lbl);
        section.append(&hbox);
    };

    let status_labels = RefCell::new(HashMap::new());
    let create_row = |frame: &GtkBox,
                      label_text: &str,
                      hint_text: Option<&str>,
                      current_val: &str,
                      is_muted: bool,
                      action: Option<&'static str>|
     -> Button {
        let hbox = GtkBox::new(Orientation::Horizontal, 12);
        hbox.set_hexpand(true);

        let text_box = GtkBox::new(Orientation::Vertical, 4);
        text_box.set_hexpand(true);

        let lbl = Label::new(Some(&t(label_text)));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        text_box.append(&lbl);

        if let Some(hint) = hint_text.filter(|_| !cfg!(target_os = "windows")) {
            let hint_lbl = Label::new(Some(&t(hint)));
            hint_lbl.set_xalign(0.0);
            hint_lbl.set_hexpand(true);
            hint_lbl.add_css_class("settings-sub-option-hint");
            text_box.append(&hint_lbl);
        }

        if let Some(action) = action {
            let status = Label::new(None);
            status.add_css_class("shortcut-row-status");
            status.set_xalign(0.0);
            status.set_wrap(true);
            status.set_visible(false);
            text_box.append(&status);
            status_labels.borrow_mut().insert(action, status);
        }

        let btn = Button::new();
        btn.add_css_class("shortcuts-record-btn");
        let empty = empty_shortcut_label();
        btn.set_label(if current_val.is_empty() {
            empty.as_str()
        } else {
            current_val
        });
        btn.set_size_request(200, -1);

        hbox.append(&text_box);
        hbox.append(&btn);

        frame.append(&build_row!(&hbox, is_muted));
        btn
    };

    // --- General Section ---
    create_header(&section, "General", "emblem-system-symbolic");
    let general_frame = build_frame();
    let open_file_btn = create_row(
        &general_frame,
        "Open File:",
        None,
        &config.shortcut_open_file,
        false,
        None,
    );
    let open_clipboard_btn = create_row(
        &general_frame,
        "Open From Clipboard:",
        None,
        &config.shortcut_open_from_clipboard,
        true,
        None,
    );
    let restore_file_btn = create_row(
        &general_frame,
        "Restore Recently Closed File:",
        None,
        &config.shortcut_restore_recently_closed,
        false,
        None,
    );
    let toggle_overlays_btn = create_row(
        &general_frame,
        "Hide/Show Overlays:",
        None,
        &config.shortcut_toggle_overlays,
        true,
        None,
    );
    section.append(&general_frame);

    // --- Screenshots Section ---
    create_header(&section, "Screenshots", "camera-photo-symbolic");
    let screenshots_frame = build_frame();
    let quick_capture_btn = create_row(
        &screenshots_frame,
        "Quick Capture:",
        Some("Open the screenshot and recording menu"),
        &config.shortcut_capture_menu,
        false,
        Some("quick_capture"),
    );
    let capture_area_btn = create_row(
        &screenshots_frame,
        "Capture Area:",
        Some("Drag to select an area of the screen"),
        &config.shortcut_capture_area,
        true,
        Some("capture_area"),
    );
    let capture_crosshair_btn = create_row(
        &screenshots_frame,
        "Crosshair Capture:",
        None,
        &config.shortcut_capture_crosshair,
        true,
        Some("capture_crosshair"),
    );
    let capture_prev_btn = create_row(
        &screenshots_frame,
        "Capture Previous Area:",
        None,
        &config.shortcut_capture_previous_area,
        false,
        Some("capture_previous_area"),
    );
    let capture_fullscreen_btn = create_row(
        &screenshots_frame,
        "Capture Full Screen:",
        None,
        &config.shortcut_capture_fullscreen,
        true,
        Some("capture_fullscreen"),
    );
    // Hide the disabled legacy direct-window shortcut. Quick Capture still
    // offers Window through its Shot menu.
    let capture_window_btn = Button::new();
    capture_window_btn.set_visible(false);
    let show_last_preview_btn = create_row(
        &screenshots_frame,
        "Show Last Preview:",
        None,
        &config.shortcut_show_last_preview,
        true,
        Some("show_last_preview"),
    );
    let save_last_capture_btn = create_row(
        &screenshots_frame,
        "Save Last Screenshot:",
        None,
        &config.shortcut_save_last_capture,
        false,
        Some("save_last_capture"),
    );
    section.append(&screenshots_frame);

    create_header(&section, "Recording", "camera-video-symbolic");
    let recording_frame = build_frame();
    let record_screen_btn = create_row(
        &recording_frame,
        "Record Screen:",
        None,
        &config.shortcut_record_screen,
        false,
        Some("record_screen"),
    );
    let recording_stop_save_btn = create_row(
        &recording_frame,
        "Stop and Save Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_stop_save,
        true,
        Some("recording_stop_save"),
    );
    // The pause, restart and marker shortcuts are handled by the Windows
    // shell only. Elsewhere their rows go into a frame that is never shown,
    // so the widgets keep one shape on every platform.
    let windows_rows = if cfg!(windows) {
        recording_frame.clone()
    } else {
        build_frame()
    };
    let recording_pause_btn = create_row(
        &windows_rows,
        "Pause or Resume Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_pause,
        true,
        Some("recording_pause"),
    );
    let recording_restart_btn = create_row(
        &windows_rows,
        "Restart Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_restart,
        true,
        Some("recording_restart"),
    );
    let recording_marker_btn = create_row(
        &windows_rows,
        "Add Recording Marker:",
        Some("Only during recording"),
        &config.shortcut_recording_marker,
        true,
        Some("recording_marker"),
    );
    section.append(&recording_frame);

    ShortcutSettingsWidgets {
        section,
        status_labels: status_labels.into_inner(),
        open_file_btn,
        open_clipboard_btn,
        restore_file_btn,
        toggle_overlays_btn,
        quick_capture_btn,
        capture_area_btn,
        capture_crosshair_btn,
        capture_prev_btn,
        capture_fullscreen_btn,
        capture_window_btn,
        show_last_preview_btn,
        save_last_capture_btn,
        record_screen_btn,
        recording_stop_save_btn,
        recording_pause_btn,
        recording_restart_btn,
        recording_marker_btn,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_shortcuts_section, compose_shortcut_label, compose_shortcut_parts,
        handle_shortcut_key, ShortcutKeyOutcome,
    };
    use crate::config::AppConfig;
    #[cfg(target_os = "windows")]
    use gtk4::prelude::BoxExt;
    use gtk4::{
        glib::object::Cast,
        prelude::{ButtonExt, WidgetExt},
        Box as GtkBox, Button, Label, Widget,
    };

    #[test]
    fn compose_shortcut_label_formats_common_combinations() {
        assert_eq!(
            compose_shortcut_label("r", true, true, false, false).as_deref(),
            Some("Ctrl+Alt+R")
        );
        assert_eq!(
            compose_shortcut_label("BackSpace", true, true, true, false).as_deref(),
            Some("Ctrl+Alt+Shift+BackSpace")
        );
        assert_eq!(
            compose_shortcut_label("Print", false, false, false, false).as_deref(),
            Some("Print")
        );
        assert_eq!(
            compose_shortcut_label("F5", false, false, false, false).as_deref(),
            Some("F5")
        );
        assert_eq!(
            compose_shortcut_label("Shift_L", false, false, true, false),
            None
        );
    }

    #[test]
    fn compose_shortcut_parts_preserves_visual_keycap_order() {
        assert_eq!(
            compose_shortcut_parts("F", true, true, false, false),
            Some(vec!["Ctrl".into(), "Alt".into(), "F".into()])
        );
    }

    #[test]
    fn shortcuts_section_hides_removed_general_rows_and_keeps_supported_ones() {
        // GTK widgets may only be built and read on the thread that initialized
        // GTK, so the section is built on the test binary's shared GTK thread
        // (see `crate::test_support`).
        let Some(labels) = crate::test_support::with_gtk(|| {
            let config = AppConfig::default();
            let widgets = build_shortcuts_section(&config);

            fn collect_labels(widget: Widget, labels: &mut Vec<String>) {
                if let Ok(button) = widget.clone().downcast::<Button>() {
                    if let Some(label) = button.label() {
                        labels.push(label.to_string());
                    }
                }
                if let Ok(label) = widget.clone().downcast::<Label>() {
                    labels.push(label.text().to_string());
                }
                if let Ok(container) = widget.clone().downcast::<GtkBox>() {
                    let mut nested: Option<Widget> = container.first_child();
                    while let Some(nested_widget) = nested {
                        collect_labels(nested_widget.clone(), labels);
                        nested = nested_widget.next_sibling();
                    }
                }
            }

            let mut child: Option<Widget> = widgets.section.first_child();
            let mut labels = Vec::new();

            while let Some(widget) = child {
                collect_labels(widget.clone(), &mut labels);
                child = widget.next_sibling();
            }

            labels
        }) else {
            eprintln!("Skipping: GTK not available (no display server)");
            return;
        };

        assert!(!labels
            .iter()
            .any(|label| label == "Use System Default Shortcuts..."));
        assert!(!labels.iter().any(|label| label == "Restore Defaults"));
        assert!(!labels.iter().any(|label| label == "Toggle Desktop Icons:"));
        assert!(!labels.iter().any(|label| label == "Pin to the Screen:"));
        assert!(labels.iter().any(|label| label == "Open File:"));
        assert!(labels.iter().any(|label| label == "Open From Clipboard:"));
        assert!(labels
            .iter()
            .any(|label| label == "Restore Recently Closed File:"));
        assert!(labels.iter().any(|label| label == "Hide/Show Overlays:"));
        assert!(labels.iter().any(|label| label == "Quick Capture:"));
        assert!(labels.iter().any(|label| label == "Capture Area:"));
        assert!(!labels.iter().any(|label| label == "Open Recording UI:"));
        #[cfg(target_os = "windows")]
        for hint in [
            "Open the screenshot and recording menu",
            "Drag to select an area of the screen",
            "Only during recording",
        ] {
            assert!(!labels.iter().any(|label| label == hint));
        }
    }

    #[test]
    fn handle_shortcut_key_ignores_keys_until_suppression_is_ready() {
        assert_eq!(
            handle_shortcut_key(false, "r", true, true, false, false),
            ShortcutKeyOutcome::Ignore
        );
        assert_eq!(
            handle_shortcut_key(true, "r", true, true, false, false),
            ShortcutKeyOutcome::Capture {
                parts: vec!["Ctrl".into(), "Alt".into(), "R".into()],
                label: "Ctrl+Alt+R".into(),
            }
        );
    }

    #[test]
    fn backspace_disables_only_without_modifiers() {
        assert_eq!(
            handle_shortcut_key(true, "BackSpace", false, false, false, false),
            ShortcutKeyOutcome::Disable
        );
        assert_eq!(
            handle_shortcut_key(true, "BackSpace", true, false, false, false),
            ShortcutKeyOutcome::Capture {
                parts: vec!["Ctrl".into(), "BackSpace".into()],
                label: "Ctrl+BackSpace".into(),
            }
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn altgr_character_uses_the_underlying_key_for_windows_shortcuts() {
        use gtk4::gdk;
        use gtk4::prelude::DisplayExtManual;

        let Some(result) = crate::test_support::with_gtk(|| {
            let display = gdk::Display::default()?;
            let mapping = display
                .map_keyval(gdk::Key::a)?
                .into_iter()
                .find(|key| key.level() == 0)?;
            let key = super::unmodified_shortcut_key(
                &display,
                mapping.keycode(),
                mapping.group(),
                gdk::Key::aacute,
            );
            let name = key.name()?;
            Some((
                compose_shortcut_label(&name, true, true, false, false),
                compose_shortcut_label(&name, true, true, true, false),
            ))
        }) else {
            return;
        };
        let Some((plain, shifted)) = result else {
            eprintln!("Skipping: no unmodified A key in the active keymap");
            return;
        };
        assert_eq!(plain.as_deref(), Some("Ctrl+Alt+A"));
        assert_eq!(shifted.as_deref(), Some("Ctrl+Alt+Shift+A"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn captured_keycaps_are_centered_in_the_available_space() {
        use gtk4::{Align, Orientation};

        let Some(result) = crate::test_support::with_gtk(|| {
            let captured = GtkBox::new(Orientation::Vertical, 18);
            let row = super::captured_keycaps_row();
            row.append(&Label::new(Some("Ctrl")));
            row.append(&Label::new(Some("+")));
            row.append(&Label::new(Some("D")));
            captured.append(&row);
            captured.measure(Orientation::Horizontal, -1);
            captured.measure(Orientation::Vertical, 320);
            captured.allocate(320, 160, -1, None);
            let bounds = row.compute_bounds(&captured).unwrap();
            (
                row.halign(),
                row.valign(),
                row.vexpands(),
                bounds.x() + bounds.width() / 2.0,
                bounds.y() + bounds.height() / 2.0,
            )
        }) else {
            return;
        };
        assert_eq!(
            (result.0, result.1, result.2),
            (Align::Center, Align::Center, true)
        );
        assert!((result.3 - 160.0).abs() < 1.0);
        assert!((result.4 - 80.0).abs() < 1.0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn shortcut_heading_titles_are_centred_on_their_icons() {
        use gtk4::{Align, Image, Orientation};

        // The heading row's spacing comes from the Windows shell and type
        // sheets, which outrank the shared one. A margin left on the title lands in
        // the row's own box and drops the title below the icon's centre while
        // `valign` still reads `Center`, so this measures a laid out row
        // instead of trusting the alignment properties.
        let Some(result) = crate::test_support::with_gtk(|| {
            crate::settings::ui_support::install_settings_css();
            crate::settings::shell::install_css();
            crate::settings::shell::install_type_css();

            let widgets = build_shortcuts_section(&AppConfig::default());
            let section = &widgets.section;
            section.measure(Orientation::Vertical, 600);
            section.allocate(600, 1600, -1, None);

            let mut child: Option<Widget> = section.first_child();
            let mut headings = Vec::new();
            while let Some(widget) = child {
                if widget.has_css_class("shortcut-heading-windows") {
                    let icon = widget.first_child().unwrap().downcast::<Image>().unwrap();
                    let label = icon.next_sibling().unwrap().downcast::<Label>().unwrap();
                    let icon_bounds = icon.compute_bounds(section).unwrap();
                    let label_bounds = label.compute_bounds(section).unwrap();
                    headings.push((
                        icon.valign(),
                        label.valign(),
                        icon_bounds.y() + icon_bounds.height() / 2.0,
                        label_bounds.y() + label_bounds.height() / 2.0,
                    ));
                }
                child = widget.next_sibling();
            }
            headings
        }) else {
            eprintln!("Skipping: GTK not available (no display server)");
            return;
        };

        assert_eq!(result.len(), 3, "expected a heading per shortcuts group");
        for (icon_align, label_align, icon_centre, label_centre) in result {
            assert_eq!(icon_align, Align::Center);
            assert_eq!(label_align, Align::Center);
            // Centring an item whose height has the other parity than the
            // row's leaves a half pixel that GTK rounds per widget, so the
            // measured centres can differ by 0.5 on a font whose line height
            // the row does not divide evenly. A whole pixel still catches the
            // margin that used to drop the title below its icon.
            assert!(
                (icon_centre - label_centre).abs() < 1.0,
                "title centre {label_centre} misses the icon centre {icon_centre}"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn capture_sheet_opens_inside_settings_and_cancels_cleanly() {
        use gtk4::{
            prelude::{GtkWindowExt, ObjectExt},
            ApplicationWindow, Overlay,
        };
        use std::{cell::Cell, rc::Rc};

        fn overlay_children(overlay: &Overlay) -> usize {
            let mut count = 0;
            let mut child = overlay.first_child();
            while let Some(widget) = child {
                count += 1;
                child = widget.next_sibling();
            }
            count
        }

        let Some(result) = crate::test_support::with_gtk(|| {
            let window = ApplicationWindow::builder().build();
            let host = Overlay::new();
            host.set_child(Some(&GtkBox::new(gtk4::Orientation::Vertical, 0)));
            window.set_child(Some(&host));

            let editing = Rc::new(Cell::new(false));
            let first = Button::with_label("Ctrl+Alt+S");
            let second = Button::with_label("Ctrl+Alt+A");
            super::install_shortcut_editor(&first, &window, &editing);
            super::install_shortcut_editor(&second, &window, &editing);

            let before = overlay_children(&host);
            first.emit_clicked();
            let open = overlay_children(&host);
            let editing_while_open = editing.get();
            // A second editor cannot stack another sheet on the first.
            second.emit_clicked();
            let after_second = overlay_children(&host);

            let card = host.last_child().unwrap();
            let header = card.first_child().unwrap();
            let cancel = header.first_child().unwrap().downcast::<Button>().unwrap();
            cancel.emit_clicked();
            let closed = overlay_children(&host);
            let editing_after_cancel = editing.get();

            first.emit_clicked();
            let reopened = overlay_children(&host);
            // `close()` returns early on a window that was never realized, so
            // raise the request a real Settings window gets from its close button.
            let _: bool = window.emit_by_name("close-request", &[]);
            let editing_after_window_close = editing.get();

            (
                before,
                open,
                editing_while_open,
                after_second,
                closed,
                editing_after_cancel,
                first.label().unwrap().to_string(),
                reopened,
                editing_after_window_close,
            )
        }) else {
            eprintln!("Skipping: GTK not available (no display server)");
            return;
        };

        assert_eq!(result.0, 1);
        assert_eq!(
            result.1, 3,
            "the scrim and the card ride on the window's overlay"
        );
        assert!(result.2);
        assert_eq!(result.3, 3);
        assert_eq!(result.4, 1, "Cancel removes the scrim and the card");
        assert!(!result.5);
        assert_eq!(result.6, "Ctrl+Alt+S", "Cancel keeps the existing shortcut");
        assert_eq!(result.7, 3);
        assert!(!result.8, "closing Settings ends the capture");
    }
}
