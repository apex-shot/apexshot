use super::windowing::prefers_dark_glass_theme;
use crate::{config::AppConfig, daemon::set_daemon_hotkey_suppressed};
use gtk4::{
    gdk, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Dialog, EventControllerKey,
    Image, Label, Orientation, ResponseType,
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
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => return None,
            }
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

fn request_hotkey_suppressed(suppressed: bool) {
    std::thread::spawn(move || {
        let _ = set_daemon_hotkey_suppressed(suppressed);
    });
}

fn install_shortcut_editor(button: &Button, parent: &ApplicationWindow) {
    let button = button.clone();
    let parent = parent.clone();
    button.clone().connect_clicked(move |_| {
        let action_name = button.label().unwrap_or_else(|| "shortcut".into());

        let dialog = Dialog::builder()
            .transient_for(&parent)
            .modal(true)
            .title("Set Shortcut")
            .default_width(400)
            .default_height(290)
            .build();
        dialog.add_css_class("shortcut-capture-dialog");
        if !prefers_dark_glass_theme() {
            dialog.add_css_class("editor-theme-light");
        }

        let header = GtkBox::new(Orientation::Horizontal, 12);
        header.set_margin_top(8);
        header.set_margin_bottom(8);
        header.set_margin_start(8);
        header.set_margin_end(8);

        let cancel_btn = Button::with_label("Cancel");
        cancel_btn.add_css_class("shortcut-capture-secondary-btn");
        let title = Label::new(Some("Set Shortcut"));
        title.add_css_class("shortcut-capture-title");
        title.set_hexpand(true);
        title.set_halign(Align::Center);
        let set_btn = Button::with_label("Set");
        set_btn.add_css_class("shortcut-capture-primary-btn");
        set_btn.set_sensitive(false);

        header.append(&cancel_btn);
        header.append(&title);
        header.append(&set_btn);

        let content = dialog.content_area();
        content.set_spacing(0);
        content.append(&header);

        let body = GtkBox::new(Orientation::Vertical, 16);
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

        let arming_box = GtkBox::new(Orientation::Vertical, 14);
        arming_box.set_vexpand(true);
        let arming_label = Label::new(Some("Preparing shortcut capture…"));
        arming_label.add_css_class("shortcut-capture-hint");
        arming_label.set_xalign(0.0);
        arming_box.append(&arming_label);
        stack.add_named(&arming_box, Some("arming"));

        let listening_box = GtkBox::new(Orientation::Vertical, 14);
        listening_box.set_vexpand(true);
        let listening_icon = Label::new(Some("⌄"));
        listening_icon.add_css_class("shortcut-capture-listening-icon");
        listening_box.append(&listening_icon);
        let listening_hint = Label::new(Some(
            "Press Esc to cancel or Backspace to disable the keyboard shortcut",
        ));
        listening_hint.set_wrap(true);
        listening_hint.set_xalign(0.0);
        listening_hint.add_css_class("shortcut-capture-hint");
        listening_box.append(&listening_hint);
        stack.add_named(&listening_box, Some("listening"));

        let captured_box = GtkBox::new(Orientation::Vertical, 18);
        captured_box.set_vexpand(true);
        let keycaps_row = GtkBox::new(Orientation::Horizontal, 10);
        keycaps_row.add_css_class("shortcut-capture-keycaps-row");
        captured_box.append(&keycaps_row);
        stack.add_named(&captured_box, Some("captured"));
        stack.set_visible_child_name("arming");
        body.append(&stack);
        content.append(&body);

        let captured_shortcut = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        let keycaps_for_keys = keycaps_row.clone();
        let stack_for_keys = stack.clone();
        let set_btn_for_keys = set_btn.clone();
        let captured_for_keys = captured_shortcut.clone();
        let suppression_ready = std::rc::Rc::new(std::cell::Cell::new(false));

        let key_controller = EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let suppression_ready_for_keys = suppression_ready.clone();
        key_controller.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let super_key = state.contains(gdk::ModifierType::SUPER_MASK)
                || state.contains(gdk::ModifierType::META_MASK);

            if let Some(name) = key.name() {
                match handle_shortcut_key(
                    suppression_ready_for_keys.get(),
                    &name,
                    ctrl,
                    alt,
                    shift,
                    super_key,
                ) {
                    ShortcutKeyOutcome::Ignore => return gtk4::glib::Propagation::Proceed,
                    ShortcutKeyOutcome::Disable => {
                        captured_for_keys.replace(Some(String::new()));
                        while let Some(child) = keycaps_for_keys.first_child() {
                            keycaps_for_keys.remove(&child);
                        }
                        let cleared = Label::new(Some("Shortcut will be disabled"));
                        cleared.add_css_class("shortcut-capture-cleared-label");
                        keycaps_for_keys.append(&cleared);
                        stack_for_keys.set_visible_child_name("captured");
                        set_btn_for_keys.set_sensitive(true);
                        return gtk4::glib::Propagation::Stop;
                    }
                    ShortcutKeyOutcome::Capture { parts, label } => {
                        captured_for_keys.replace(Some(label));
                        while let Some(child) = keycaps_for_keys.first_child() {
                            keycaps_for_keys.remove(&child);
                        }

                        for (idx, part) in parts.iter().enumerate() {
                            let keycap = Label::new(Some(part));
                            keycap.add_css_class("shortcut-capture-keycap");
                            keycaps_for_keys.append(&keycap);
                            if idx + 1 != parts.len() {
                                let plus = Label::new(Some("+"));
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
        dialog.add_controller(key_controller);

        let dialog_for_cancel = dialog.clone();
        cancel_btn.connect_clicked(move |_| dialog_for_cancel.response(ResponseType::Cancel));
        let dialog_for_set = dialog.clone();
        set_btn.connect_clicked(move |_| dialog_for_set.response(ResponseType::Accept));

        let captured_for_response = captured_shortcut.clone();
        let button_for_response = button.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(text) = captured_for_response.borrow().clone() {
                    if text.is_empty() {
                        button_for_response.set_label("Record shortcut");
                    } else {
                        button_for_response.set_label(&text);
                    }
                }
            }
            request_hotkey_suppressed(false);
            dialog.close();
        });

        dialog.connect_close_request(|_| {
            request_hotkey_suppressed(false);
            gtk4::glib::Propagation::Proceed
        });

        let dialog_for_present = dialog.clone();
        let stack_for_present = stack.clone();
        let suppression_ready_for_present = suppression_ready.clone();
        dialog_for_present.present();
        stack_for_present.set_visible_child_name("listening");
        suppression_ready_for_present.set(true);
        request_hotkey_suppressed(true);
    });
}

pub fn install_shortcut_editors(widgets: &ShortcutSettingsWidgets, parent: &ApplicationWindow) {
    for button in [
        &widgets.open_file_btn,
        &widgets.open_clipboard_btn,
        &widgets.restore_file_btn,
        &widgets.toggle_overlays_btn,
        &widgets.capture_area_btn,
        &widgets.capture_crosshair_btn,
        &widgets.capture_prev_btn,
        &widgets.capture_fullscreen_btn,
        &widgets.capture_window_btn,
        &widgets.show_last_preview_btn,
        &widgets.open_recording_ui_btn,
        &widgets.record_screen_btn,
        &widgets.recording_pause_resume_btn,
        &widgets.recording_stop_save_btn,
        &widgets.recording_restart_btn,
        &widgets.recording_discard_btn,
    ] {
        install_shortcut_editor(button, parent);
    }
}

pub struct ShortcutSettingsWidgets {
    pub section: GtkBox,
    pub open_file_btn: Button,
    pub open_clipboard_btn: Button,
    pub restore_file_btn: Button,
    pub toggle_overlays_btn: Button,
    pub capture_area_btn: Button,
    pub capture_crosshair_btn: Button,
    pub capture_prev_btn: Button,
    pub capture_fullscreen_btn: Button,
    pub capture_window_btn: Button,
    pub show_last_preview_btn: Button,
    pub open_recording_ui_btn: Button,
    pub record_screen_btn: Button,
    pub recording_pause_resume_btn: Button,
    pub recording_stop_save_btn: Button,
    pub recording_restart_btn: Button,
    pub recording_discard_btn: Button,
}

pub fn build_shortcuts_section(config: &AppConfig) -> ShortcutSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);

    let tip = Label::new(Some(
        "Shortcuts set here are the same hotkeys ApexShot uses. If one does not work, your desktop environment may already be using it. Open your system keyboard settings and disable conflicting shortcuts if you want to reuse them in ApexShot.",
    ));
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
        hbox.set_margin_bottom(8);
        hbox.set_margin_top(8);

        let img = Image::from_icon_name(icon);
        img.set_pixel_size(20);

        let lbl = Label::new(Some(label_text));
        lbl.add_css_class("settings-group-title");

        hbox.append(&img);
        hbox.append(&lbl);
        section.append(&hbox);
    };

    let create_row = |frame: &GtkBox,
                      label_text: &str,
                      hint_text: Option<&str>,
                      current_val: &str,
                      is_muted: bool|
     -> Button {
        let hbox = GtkBox::new(Orientation::Horizontal, 12);
        hbox.set_hexpand(true);

        let text_box = GtkBox::new(Orientation::Vertical, 4);
        text_box.set_hexpand(true);

        let lbl = Label::new(Some(label_text));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        text_box.append(&lbl);

        if let Some(hint) = hint_text {
            let hint_lbl = Label::new(Some(hint));
            hint_lbl.set_xalign(0.0);
            hint_lbl.set_hexpand(true);
            hint_lbl.add_css_class("settings-sub-option-hint");
            text_box.append(&hint_lbl);
        }

        let btn = Button::new();
        btn.add_css_class("shortcuts-record-btn");
        btn.set_label(if current_val.is_empty() {
            "Record shortcut"
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
    );
    let open_clipboard_btn = create_row(
        &general_frame,
        "Open From Clipboard:",
        None,
        &config.shortcut_open_from_clipboard,
        true,
    );
    let restore_file_btn = create_row(
        &general_frame,
        "Restore Recently Closed File:",
        None,
        &config.shortcut_restore_recently_closed,
        false,
    );
    let toggle_overlays_btn = create_row(
        &general_frame,
        "Hide/Show Overlays:",
        None,
        &config.shortcut_toggle_overlays,
        true,
    );
    section.append(&general_frame);

    // --- Screenshots Section ---
    create_header(&section, "Screenshots", "camera-photo-symbolic");
    let screenshots_frame = build_frame();
    let capture_area_btn = create_row(
        &screenshots_frame,
        "Capture Area:",
        None,
        &config.shortcut_capture_area,
        false,
    );
    let capture_crosshair_btn = create_row(
        &screenshots_frame,
        "Crosshair Capture:",
        None,
        &config.shortcut_capture_crosshair,
        true,
    );
    let capture_prev_btn = create_row(
        &screenshots_frame,
        "Capture Previous Area:",
        None,
        &config.shortcut_capture_previous_area,
        false,
    );
    let capture_fullscreen_btn = create_row(
        &screenshots_frame,
        "Capture Full Screen:",
        None,
        &config.shortcut_capture_fullscreen,
        true,
    );
    let capture_window_btn = create_row(
        &screenshots_frame,
        "Capture Window:",
        None,
        &config.shortcut_capture_window,
        false,
    );
    let show_last_preview_btn = create_row(
        &screenshots_frame,
        "Show Last Preview:",
        None,
        &config.shortcut_show_last_preview,
        true,
    );
    section.append(&screenshots_frame);

    create_header(&section, "Recording", "camera-video-symbolic");
    let recording_frame = build_frame();
    let open_recording_ui_btn = create_row(
        &recording_frame,
        "Open Recording UI:",
        None,
        &config.shortcut_open_recording_ui,
        false,
    );
    let record_screen_btn = create_row(
        &recording_frame,
        "Record Screen:",
        None,
        &config.shortcut_record_screen,
        true,
    );
    let recording_pause_resume_btn = create_row(
        &recording_frame,
        "Pause/Resume Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_pause_resume,
        false,
    );
    let recording_stop_save_btn = create_row(
        &recording_frame,
        "Stop and Save Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_stop_save,
        true,
    );
    let recording_restart_btn = create_row(
        &recording_frame,
        "Restart Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_restart,
        false,
    );
    let recording_discard_btn = create_row(
        &recording_frame,
        "Discard Recording:",
        Some("Only during recording"),
        &config.shortcut_recording_discard,
        true,
    );
    section.append(&recording_frame);

    ShortcutSettingsWidgets {
        section,
        open_file_btn,
        open_clipboard_btn,
        restore_file_btn,
        toggle_overlays_btn,
        capture_area_btn,
        capture_crosshair_btn,
        capture_prev_btn,
        capture_fullscreen_btn,
        capture_window_btn,
        show_last_preview_btn,
        open_recording_ui_btn,
        record_screen_btn,
        recording_pause_resume_btn,
        recording_stop_save_btn,
        recording_restart_btn,
        recording_discard_btn,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_shortcuts_section, compose_shortcut_label, compose_shortcut_parts,
        handle_shortcut_key, ShortcutKeyOutcome,
    };
    use crate::config::AppConfig;
    use gtk4::{
        glib::object::Cast,
        prelude::{ButtonExt, WidgetExt},
        Box as GtkBox, Button, Label, Widget,
    };
    use std::sync::Once;

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
        use std::sync::atomic::{AtomicBool, Ordering};
        static GTK_AVAILABLE: AtomicBool = AtomicBool::new(false);
        static GTK_INIT: Once = Once::new();
        GTK_INIT.call_once(|| {
            GTK_AVAILABLE.store(gtk4::init().is_ok(), Ordering::Relaxed);
        });

        if !GTK_AVAILABLE.load(Ordering::Relaxed) {
            eprintln!("Skipping: GTK not available (no display server)");
            return;
        }

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
}
