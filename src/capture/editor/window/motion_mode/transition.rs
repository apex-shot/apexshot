use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, Label, Orientation,
    Overlay, Stack,
};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::state::EditorState;
use crate::config::{load_config, save_config};
use crate::i18n::t;

use super::{MotionSession, MOTION_PAGE, STATIC_PAGE};

pub(in crate::capture::editor::window) struct MotionModeChrome {
    pub mode_stack: Stack,
    pub canvas_stack: Stack,
    pub bottom_left_stack: Stack,
    pub motion_control: GtkBox,
    pub history_control: GtkBox,
    pub inspector_tabs: GtkBox,
    pub motion_tabs: GtkBox,
    pub inspector_stack: Stack,
    pub motion_tab_btn: Button,
    pub appearance_tab_btn: Button,
    pub watermark_tab_btn: Button,
}

pub(in crate::capture::editor::window) fn apply_editor_mode(
    chrome: &MotionModeChrome,
    motion: bool,
    last_inspector: &str,
) {
    let page = if motion { MOTION_PAGE } else { STATIC_PAGE };
    chrome.mode_stack.set_visible_child_name(page);
    chrome.canvas_stack.set_visible_child_name(page);
    chrome.bottom_left_stack.set_visible(!motion);
    chrome.motion_control.set_visible(!motion);
    chrome.history_control.set_visible(!motion);
    chrome.inspector_tabs.set_visible(!motion);
    chrome.motion_tabs.set_visible(motion);
    if motion {
        chrome.inspector_stack.set_visible_child_name(MOTION_PAGE);
        chrome.motion_tab_btn.add_css_class("active-tool");
        chrome.appearance_tab_btn.remove_css_class("active-tool");
        chrome.watermark_tab_btn.remove_css_class("active-tool");
    } else {
        chrome
            .inspector_stack
            .set_visible_child_name(last_inspector);
    }
}

pub(in crate::capture::editor::window) fn annotations_need_snapshot(state: &EditorState) -> bool {
    !state.actions.is_empty() || state.crop_selection.is_some()
}

#[derive(Clone, Copy)]
enum ConfirmKind {
    ToMotion,
    ToStatic,
}

pub(in crate::capture::editor::window) fn request_enter_motion(
    window: &ApplicationWindow,
    confirm_overlay: &GtkBox,
    _session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    empty_drop_zone: bool,
    enter: Rc<dyn Fn()>,
) {
    if empty_drop_zone {
        return;
    }
    let needs_confirm = {
        let guard = state.lock().unwrap();
        annotations_need_snapshot(&guard) && !load_config().skip_static_to_motion_confirm
    };
    if needs_confirm {
        show_confirm(
            window,
            confirm_overlay,
            ConfirmKind::ToMotion,
            Rc::new({
                let enter = enter.clone();
                move || enter()
            }),
        );
        return;
    }
    enter();
}

pub(in crate::capture::editor::window) fn request_leave_motion(
    window: &ApplicationWindow,
    confirm_overlay: &GtkBox,
    session: &MotionSession,
    leave: Rc<dyn Fn()>,
) {
    if session.has_segments() && !load_config().skip_motion_to_static_confirm {
        show_confirm(window, confirm_overlay, ConfirmKind::ToStatic, leave);
        return;
    }
    leave();
}

fn show_confirm(
    _window: &ApplicationWindow,
    overlay: &GtkBox,
    kind: ConfirmKind,
    on_continue: Rc<dyn Fn()>,
) {
    while let Some(child) = overlay.first_child() {
        overlay.remove(&child);
    }

    let (title, body, skip_field) = match kind {
        ConfirmKind::ToMotion => (
            t("Annotations will be applied"),
            t("Motion will play a snapshot of your annotations. Returning to Static restores the live tools. Your original image is not flattened until Done."),
            "static_to_motion",
        ),
        ConfirmKind::ToStatic => (
            t("Motion effects will be cleared"),
            t("Motion effect segments can't be edited in Static mode and will be cleared if you continue."),
            "motion_to_static",
        ),
    };

    let card = GtkBox::new(Orientation::Vertical, 12);
    card.add_css_class("editor-motion-confirm-card");
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_hexpand(false);

    let title_label = Label::new(Some(&title));
    title_label.add_css_class("editor-inspector-title");
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.set_max_width_chars(44);

    let body_label = Label::new(Some(&body));
    body_label.add_css_class("editor-select-inspector-hint");
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    body_label.set_max_width_chars(44);

    let skip = CheckButton::with_label(&t("Don't show this again"));
    skip.add_css_class("editor-motion-confirm-skip");

    let buttons = GtkBox::new(Orientation::Horizontal, 8);
    buttons.set_halign(Align::End);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.set_has_frame(false);
    cancel.add_css_class("editor-sidebar-action-button");
    let cont = Button::with_label(&t("Continue"));
    cont.set_has_frame(false);
    cont.add_css_class("editor-done-button");
    buttons.append(&cancel);
    buttons.append(&cont);

    card.append(&title_label);
    card.append(&body_label);
    card.append(&skip);
    card.append(&buttons);
    overlay.append(&card);
    overlay.set_visible(true);

    cancel.connect_clicked({
        let overlay = overlay.clone();
        move |_| overlay.set_visible(false)
    });
    cont.connect_clicked({
        let overlay = overlay.clone();
        let skip = skip.clone();
        let skip_field = skip_field.to_string();
        move |_| {
            if skip.is_active() {
                let mut config = load_config();
                match skip_field.as_str() {
                    "static_to_motion" => config.skip_static_to_motion_confirm = true,
                    _ => config.skip_motion_to_static_confirm = true,
                }
                let _ = save_config(&config);
            }
            overlay.set_visible(false);
            on_continue();
        }
    });
}

pub(in crate::capture::editor::window) fn install_confirm_overlay(
    root_overlay: &Overlay,
    confirm: &GtkBox,
) {
    root_overlay.add_overlay(confirm);
    root_overlay.set_clip_overlay(confirm, true);
}
