//! Auto-save for the settings window.
//!
//! The Windows shell has no Save button: a change is written a moment after
//! the person stops making it. Debouncing keeps a typed path from writing a
//! file per keystroke, and a save that lands while another is still running
//! queues exactly one follow-up.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::{glib, prelude::CheckButtonExt, ApplicationWindow, Label};

use super::actions::{save_settings, SaveInputs};
use super::{format_save_error, rebuild_settings_window, show_settings_toast, SettingsToastKind};
use crate::i18n::{t, tfmt};

/// How long the person has to stop editing before the change is written.
const DEBOUNCE: Duration = Duration::from_millis(400);

pub struct AutoSaver {
    window: ApplicationWindow,
    inputs: Rc<SaveInputs>,
    toast: Label,
    toast_generation: Rc<Cell<u32>>,
    pending: RefCell<Option<glib::SourceId>>,
    dirty: Cell<bool>,
    busy: Cell<bool>,
    queued: Cell<bool>,
}

impl AutoSaver {
    pub fn new(
        window: ApplicationWindow,
        inputs: Rc<SaveInputs>,
        toast: Label,
        toast_generation: Rc<Cell<u32>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            window,
            inputs,
            toast,
            toast_generation,
            pending: RefCell::new(None),
            dirty: Cell::new(false),
            busy: Cell::new(false),
            queued: Cell::new(false),
        })
    }

    /// The callback the controls call on every edit.
    pub fn mark_dirty(self: &Rc<Self>) -> Rc<dyn Fn()> {
        let saver = Rc::clone(self);
        Rc::new(move || saver.schedule())
    }

    fn schedule(self: &Rc<Self>) {
        self.dirty.set(true);
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        let saver = Rc::clone(self);
        let source = glib::timeout_add_local_once(DEBOUNCE, move || {
            saver.pending.borrow_mut().take();
            saver.save();
        });
        *self.pending.borrow_mut() = Some(source);
    }

    fn save(self: &Rc<Self>) {
        if self.busy.get() {
            self.queued.set(true);
            return;
        }
        if !self.dirty.get() {
            return;
        }
        self.busy.set(true);
        self.dirty.set(false);

        let saver = Rc::clone(self);
        glib::idle_add_local_once(move || {
            let outcome = save_settings(&saver.inputs);
            match outcome {
                Ok(outcome) => {
                    if outcome.language_changed {
                        crate::i18n::apply_gtk_direction(&crate::config::load_config().ui_language);
                    }
                    if outcome.language_changed || outcome.theme_changed {
                        saver.busy.set(false);
                        rebuild_settings_window(&saver.window);
                        return;
                    }
                    show_settings_toast(
                        &saver.toast,
                        &saver.toast_generation,
                        &t("Saved"),
                        SettingsToastKind::Success,
                        Some(Duration::from_millis(1400)),
                    );
                }
                Err(error) => {
                    let saved_startup = super::windowing::autostart_entry_exists()
                        .unwrap_or_else(|_| crate::config::load_config().start_at_login);
                    if saver.inputs.start_at_login.is_active() != saved_startup {
                        saver.inputs.start_at_login.set_active(saved_startup);
                    }
                    let message = format_save_error(&error);
                    eprintln!("[settings] Save failed: {error:#}");
                    show_settings_toast(
                        &saver.toast,
                        &saver.toast_generation,
                        &tfmt("Save failed: {message}", &[("message", &message)]),
                        SettingsToastKind::Error,
                        Some(Duration::from_secs(4)),
                    );
                }
            }
            saver.busy.set(false);
            if saver.queued.replace(false) && saver.dirty.get() {
                saver.schedule();
            }
        });
    }

    /// Write any waiting change now, for a window that is closing.
    pub fn flush(&self) {
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        if self.busy.get() || !self.dirty.get() {
            return;
        }
        self.dirty.set(false);
        if let Err(error) = save_settings(&self.inputs) {
            eprintln!("[settings] Save on close failed: {error:#}");
        }
    }
}
