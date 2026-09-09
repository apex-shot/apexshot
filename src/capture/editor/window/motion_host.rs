use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{prelude::*, ApplicationWindow, Box as GtkBox, Button, Overlay, Stack};

use crate::capture::editor::state::EditorState;

use super::chrome::WindowChrome;
use super::motion_mode::{self, MotionModeChrome, MotionModeParts, MotionSession};

pub(super) struct MotionHost {
    pub(super) parts: MotionModeParts,
    session: Rc<MotionSession>,
    last_inspector: Rc<RefCell<String>>,
    in_motion: Rc<Cell<bool>>,
}

pub(super) struct MotionHostInstallInputs<'a> {
    pub window: &'a ApplicationWindow,
    pub root_overlay: &'a Overlay,
    pub canvas_with_toolbar: &'a Overlay,
    pub canvas_stack: &'a Stack,
    pub window_chrome: WindowChrome,
    pub inspector_tabs: &'a GtkBox,
    pub motion_tabs: &'a GtkBox,
    pub inspector_stack: &'a Stack,
    pub motion_tab_btn: &'a Button,
    pub appearance_tab_btn: &'a Button,
    pub watermark_tab_btn: &'a Button,
    pub state: &'a Arc<Mutex<EditorState>>,
    pub empty_drop_zone: bool,
}

impl MotionHost {
    pub(super) fn new(
        window: &ApplicationWindow,
        prefers_dark: bool,
        empty_drop_zone: bool,
    ) -> Self {
        let (parts, session) = motion_mode::build_motion_mode(window, prefers_dark);
        if empty_drop_zone {
            parts.shell.motion_btn.set_sensitive(false);
        }

        Self {
            parts,
            session: Rc::new(session),
            last_inspector: Rc::new(RefCell::new(String::from("placeholder"))),
            in_motion: Rc::new(Cell::new(false)),
        }
    }

    pub(super) fn last_inspector(&self) -> Rc<RefCell<String>> {
        self.last_inspector.clone()
    }

    pub(super) fn in_motion(&self) -> Rc<Cell<bool>> {
        self.in_motion.clone()
    }

    pub(super) fn export_callback(&self, path: PathBuf) -> Rc<dyn Fn() -> Result<PathBuf, String>> {
        let session = self.session.clone();
        Rc::new(move || session.export_mp4(&path))
    }

    pub(super) fn install(&self, input: MotionHostInstallInputs<'_>) {
        let MotionHostInstallInputs {
            window,
            root_overlay,
            canvas_with_toolbar,
            canvas_stack,
            window_chrome,
            inspector_tabs,
            motion_tabs,
            inspector_stack,
            motion_tab_btn,
            appearance_tab_btn,
            watermark_tab_btn,
            state,
            empty_drop_zone,
        } = input;

        // Add this after the full-width drag chrome. GTK overlays are hit-tested
        // in stacking order; placing the Motion tool pill above the drag strip is
        // what keeps both buttons clickable, just like the static toolbar tools.
        canvas_with_toolbar.add_overlay(motion_tabs);
        canvas_with_toolbar.set_clip_overlay(motion_tabs, true);
        motion_mode::install_confirm_overlay(root_overlay, &self.parts.shell.confirm_overlay);

        let motion_chrome = Rc::new(MotionModeChrome {
            mode_stack: window_chrome.mode_stack,
            canvas_stack: canvas_stack.clone(),
            bottom_left_stack: window_chrome.bottom_left_stack,
            motion_control: window_chrome.motion_control,
            history_control: window_chrome.history_control,
            inspector_tabs: inspector_tabs.clone(),
            motion_tabs: motion_tabs.clone(),
            inspector_stack: inspector_stack.clone(),
            motion_tab_btn: motion_tab_btn.clone(),
            appearance_tab_btn: appearance_tab_btn.clone(),
            watermark_tab_btn: watermark_tab_btn.clone(),
        });
        motion_tab_btn.connect_clicked({
            let inspector_stack = inspector_stack.clone();
            let motion_tab_btn = motion_tab_btn.clone();
            let appearance_tab_btn = appearance_tab_btn.clone();
            let watermark_tab_btn = watermark_tab_btn.clone();
            move |_| {
                inspector_stack.set_visible_child_name("motion");
                motion_tab_btn.add_css_class("active-tool");
                appearance_tab_btn.remove_css_class("active-tool");
                watermark_tab_btn.remove_css_class("active-tool");
            }
        });
        appearance_tab_btn.connect_clicked({
            let inspector_stack = inspector_stack.clone();
            let motion_tab_btn = motion_tab_btn.clone();
            let appearance_tab_btn = appearance_tab_btn.clone();
            let watermark_tab_btn = watermark_tab_btn.clone();
            move |_| {
                inspector_stack.set_visible_child_name("motion-appearance");
                appearance_tab_btn.add_css_class("active-tool");
                motion_tab_btn.remove_css_class("active-tool");
                watermark_tab_btn.remove_css_class("active-tool");
            }
        });
        watermark_tab_btn.connect_clicked({
            let inspector_stack = inspector_stack.clone();
            let motion_tab_btn = motion_tab_btn.clone();
            let appearance_tab_btn = appearance_tab_btn.clone();
            let watermark_tab_btn = watermark_tab_btn.clone();
            move |_| {
                inspector_stack.set_visible_child_name("motion-watermark");
                watermark_tab_btn.add_css_class("active-tool");
                motion_tab_btn.remove_css_class("active-tool");
                appearance_tab_btn.remove_css_class("active-tool");
            }
        });
        motion_mode::wire_motion_controls(
            &self.parts,
            self.session.as_ref(),
            motion_chrome.clone(),
            self.last_inspector.clone(),
            self.in_motion.clone(),
        );

        let enter_motion = {
            let state = state.clone();
            let session = self.session.clone();
            let preview = self.parts.shell.preview.clone();
            let ruler = self.parts.timeline.ruler.clone();
            let motion_track = self.parts.timeline.motion_track.clone();
            let text_track = self.parts.timeline.text_track.clone();
            let playhead_overlay = self.parts.timeline.playhead_overlay.clone();
            let motion_chrome = motion_chrome.clone();
            let last_inspector = self.last_inspector.clone();
            let in_motion = self.in_motion.clone();
            let duration_slider = self.parts.shared.duration_slider.clone();
            let duration_value = self.parts.shared.duration_value.clone();
            Rc::new(move || {
                session.capture_snapshot(&state.lock().unwrap());
                let duration = session.duration();
                duration_slider.set_value(duration);
                duration_value.set_label(&format!("{duration:.1}s"));
                motion_mode::apply_editor_mode(
                    &motion_chrome,
                    true,
                    last_inspector.borrow().as_str(),
                );
                in_motion.set(true);
                preview.queue_draw();
                ruler.queue_draw();
                motion_track.queue_draw();
                text_track.queue_draw();
                playhead_overlay.queue_draw();
            }) as Rc<dyn Fn()>
        };
        let leave_motion = {
            let session = self.session.clone();
            let motion_chrome = motion_chrome.clone();
            let last_inspector = self.last_inspector.clone();
            let in_motion = self.in_motion.clone();
            Rc::new(move || {
                session.clear_snapshot();
                motion_mode::apply_editor_mode(
                    &motion_chrome,
                    false,
                    last_inspector.borrow().as_str(),
                );
                in_motion.set(false);
            }) as Rc<dyn Fn()>
        };

        self.parts.shell.motion_btn.connect_clicked({
            let window = window.clone();
            let confirm = self.parts.shell.confirm_overlay.clone();
            let state = state.clone();
            let session = self.session.clone();
            let enter_motion = enter_motion.clone();
            move |_| {
                motion_mode::request_enter_motion(
                    &window,
                    &confirm,
                    session.as_ref(),
                    &state,
                    empty_drop_zone,
                    enter_motion.clone(),
                );
            }
        });
        self.parts.shell.static_btn.connect_clicked({
            let window = window.clone();
            let confirm = self.parts.shell.confirm_overlay.clone();
            let session = self.session.clone();
            let leave_motion = leave_motion.clone();
            move |_| {
                motion_mode::request_leave_motion(
                    &window,
                    &confirm,
                    session.as_ref(),
                    leave_motion.clone(),
                );
            }
        });
    }
}
