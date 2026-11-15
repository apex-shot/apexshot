use super::dialogs;
use gtk4::{glib, prelude::*, ApplicationWindow, Widget};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) type FlowActive = Rc<Cell<bool>>;
pub(super) type FlowSlot = Rc<RefCell<Option<ActiveFlow>>>;

pub(super) struct ActiveFlow {
    active: FlowActive,
    window: glib::WeakRef<ApplicationWindow>,
    was_sensitive: bool,
    focus: glib::WeakRef<Widget>,
}

impl ActiveFlow {
    pub(super) fn begin(window: &ApplicationWindow, active: &FlowActive) -> Option<FlowSlot> {
        if active.replace(true) {
            return None;
        }
        let was_sensitive = dialogs::editor_root(window)
            .map(|root| root.is_sensitive())
            .unwrap_or(true);
        let focus = gtk4::prelude::GtkWindowExt::focus(window)
            .map(|widget: Widget| widget.downgrade())
            .unwrap_or_default();
        dialogs::set_editor_root_sensitive(window, false);
        Some(Rc::new(RefCell::new(Some(Self {
            active: active.clone(),
            window: window.downgrade(),
            was_sensitive,
            focus,
        }))))
    }
}

impl Drop for ActiveFlow {
    fn drop(&mut self) {
        if let Some(window) = self.window.upgrade() {
            dialogs::set_editor_root_sensitive(&window, self.was_sensitive);
        }
        if let Some(focus) = self.focus.upgrade() {
            if focus.is_sensitive() {
                focus.grab_focus();
            }
        }
        self.active.set(false);
    }
}

pub(super) fn release(slot: &FlowSlot) {
    slot.borrow_mut().take();
}
