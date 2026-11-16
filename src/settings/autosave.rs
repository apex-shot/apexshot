use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::{glib, prelude::*, ApplicationWindow, Box as GtkBox, Label};

use super::{format_save_error, show_settings_toast};
use crate::i18n::{t, tfmt};

const DEBOUNCE: Duration = Duration::from_millis(400);

pub(crate) enum AutoSaveResult {
    Saved {
        language_changed: bool,
        theme_changed: bool,
    },
    Failed(anyhow::Error),
}

pub(crate) struct AutoSaver {
    window: glib::WeakRef<ApplicationWindow>,
    toast: Label,
    toast_generation: Rc<Cell<u32>>,
    error_bar: GtkBox,
    error_label: Label,
    save: Rc<dyn Fn() -> AutoSaveResult>,
    finish: Rc<dyn Fn()>,
    rebuild: Rc<dyn Fn(&ApplicationWindow)>,
    pending: RefCell<Option<glib::SourceId>>,
    revision: Cell<u64>,
    persisted: Cell<u64>,
}

impl Drop for AutoSaver {
    fn drop(&mut self) {
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
    }
}

impl AutoSaver {
    pub(crate) fn new(
        window: &ApplicationWindow,
        toast: Label,
        toast_generation: Rc<Cell<u32>>,
        error_bar: GtkBox,
        error_label: Label,
        save: Rc<dyn Fn() -> AutoSaveResult>,
        finish: Rc<dyn Fn()>,
        rebuild: Rc<dyn Fn(&ApplicationWindow)>,
    ) -> Rc<Self> {
        Rc::new(Self {
            window: window.downgrade(),
            toast,
            toast_generation,
            error_bar,
            error_label,
            save,
            finish,
            rebuild,
            pending: RefCell::new(None),
            revision: Cell::new(0),
            persisted: Cell::new(0),
        })
    }

    pub(crate) fn mark_dirty(self: &Rc<Self>) -> Rc<dyn Fn()> {
        let saver = Rc::downgrade(self);
        Rc::new(move || {
            if let Some(saver) = saver.upgrade() {
                saver.note_edit();
            }
        })
    }

    fn note_edit(self: &Rc<Self>) {
        self.revision.set(self.revision.get().wrapping_add(1));
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        let saver = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(DEBOUNCE, move || {
            if let Some(saver) = saver.upgrade() {
                saver.pending.borrow_mut().take();
                saver.run_save();
            }
        });
        *self.pending.borrow_mut() = Some(source);
    }

    fn dirty(&self) -> bool {
        self.revision.get() != self.persisted.get()
    }

    fn run_save(self: &Rc<Self>) {
        let revision = self.revision.get();
        if revision == self.persisted.get() {
            return;
        }
        match (self.save)() {
            AutoSaveResult::Saved {
                language_changed,
                theme_changed,
            } => {
                self.persisted.set(revision);
                self.error_bar.set_visible(false);
                if language_changed {
                    crate::i18n::apply_gtk_direction(&crate::config::load_config().ui_language);
                }
                if language_changed || theme_changed {
                    if let Some(window) = self.window.upgrade() {
                        (self.rebuild)(&window);
                    }
                    return;
                }
                show_settings_toast(
                    &self.toast,
                    &self.toast_generation,
                    &t("Saved"),
                    Some(Duration::from_millis(1400)),
                );
                if self.dirty() {
                    self.note_edit();
                }
            }
            AutoSaveResult::Failed(error) => {
                let message = format_save_error(&error);
                eprintln!("[settings] Save failed: {error:#}");
                self.show_error(&tfmt("Save failed: {message}", &[("message", &message)]));
            }
        }
    }

    fn show_error(&self, message: &str) {
        self.error_label.set_text(message);
        self.error_bar.set_visible(true);
    }

    pub(crate) fn retry(self: &Rc<Self>) {
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        self.run_save();
    }

    pub(crate) fn flush(&self) -> Result<(), anyhow::Error> {
        if let Some(source) = self.pending.borrow_mut().take() {
            source.remove();
        }
        let revision = self.revision.get();
        if revision == self.persisted.get() {
            (self.finish)();
            return Ok(());
        }
        match (self.save)() {
            AutoSaveResult::Saved { .. } => {
                self.persisted.set(revision);
                self.error_bar.set_visible(false);
                (self.finish)();
                Ok(())
            }
            AutoSaveResult::Failed(error) => {
                eprintln!("[settings] Save on close failed: {error:#}");
                let message = format_save_error(&error);
                self.show_error(&tfmt("Save failed: {message}", &[("message", &message)]));
                Err(error)
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use gtk4::Button;

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    thread_local! {
        static SAVER: RefCell<Option<Rc<AutoSaver>>> = const { RefCell::new(None) };
    }

    fn new_saver(save: Rc<dyn Fn() -> AutoSaveResult>) {
        new_saver_with_finish(save, Rc::new(|| {}));
    }

    fn new_saver_with_finish(save: Rc<dyn Fn() -> AutoSaveResult>, finish: Rc<dyn Fn()>) {
        SAVER.with(|slot| {
            if let Some(old) = slot.borrow_mut().take() {
                if let Some(source) = old.pending.borrow_mut().take() {
                    source.remove();
                }
            }
            let app = crate::test_support::test_app();
            let window = ApplicationWindow::builder().application(&app).build();
            let toast = Label::new(None);
            let error_bar = GtkBox::new(gtk4::Orientation::Horizontal, 8);
            let error_label = Label::new(None);
            let retry = Button::with_label("Retry");
            error_bar.append(&error_label);
            error_bar.append(&retry);
            let saver = AutoSaver::new(
                &window,
                toast,
                Rc::new(Cell::new(0)),
                error_bar,
                error_label,
                save,
                finish,
                Rc::new(|_| {}),
            );
            *slot.borrow_mut() = Some(saver);
        });
    }

    fn gtk_available() -> bool {
        if crate::test_support::with_gtk(|| ()).is_none() {
            eprintln!("skipping: no GTK display available");
            return false;
        }
        true
    }

    fn saved() -> AutoSaveResult {
        AutoSaveResult::Saved {
            language_changed: false,
            theme_changed: false,
        }
    }

    #[test]
    fn rapid_edits_collapse_into_one_save() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = {
            let calls = std::sync::Arc::clone(&calls);
            move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                saved()
            }
        };
        assert!(crate::test_support::with_gtk(move || {
            new_saver(Rc::new(counting));
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                let dirty = saver.mark_dirty();
                dirty();
                dirty();
                dirty();
            });
        })
        .is_some());
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn nothing_is_written_without_an_edit() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = {
            let calls = std::sync::Arc::clone(&calls);
            move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                saved()
            }
        };
        assert!(crate::test_support::with_gtk(move || {
            new_saver(Rc::new(counting));
        })
        .is_some());
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn flush_writes_pending_changes_before_the_debounce() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = {
            let calls = std::sync::Arc::clone(&calls);
            move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                saved()
            }
        };
        let result = crate::test_support::with_gtk(move || {
            new_saver(Rc::new(counting));
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                saver.mark_dirty()();
                saver.flush()
            })
        });
        assert!(matches!(result, Some(Ok(()))));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_flush_stays_dirty_and_retry_succeeds() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let flaky = {
            let calls = std::sync::Arc::clone(&calls);
            move || {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    AutoSaveResult::Failed(anyhow::anyhow!("fixture failure"))
                } else {
                    saved()
                }
            }
        };
        let result = crate::test_support::with_gtk(move || {
            new_saver(Rc::new(flaky));
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                saver.mark_dirty()();
                let first = saver.flush().is_err();
                let second = saver.flush().is_ok();
                let third = saver.flush().is_ok();
                (first, second, third)
            })
        });
        assert_eq!(result, Some((true, true, true)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn an_edit_inside_the_save_is_not_cleared_by_it() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let nested = {
            let calls = std::sync::Arc::clone(&calls);
            move || {
                let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call == 0 {
                    SAVER.with(|slot| {
                        if let Some(saver) = slot.borrow().as_ref() {
                            saver.note_edit();
                        }
                    });
                }
                saved()
            }
        };
        assert!(crate::test_support::with_gtk(move || {
            new_saver(Rc::new(nested));
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                saver.mark_dirty()();
                saver.run_save();
                assert!(saver.dirty());
            });
        })
        .is_some());
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn a_clean_close_still_finishes_runtime_updates() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = {
            let finished = std::sync::Arc::clone(&finished);
            move || {
                finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let result = crate::test_support::with_gtk(move || {
            new_saver_with_finish(Rc::new(saved), Rc::new(counting));
            SAVER.with(|slot| slot.borrow().as_ref().unwrap().flush())
        });
        assert!(matches!(result, Some(Ok(()))));
        assert_eq!(finished.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_dirty_close_saves_before_finishing() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let saving = {
            let order = std::sync::Arc::clone(&order);
            move || {
                order.lock().unwrap().push("save");
                saved()
            }
        };
        let finishing = {
            let order = std::sync::Arc::clone(&order);
            move || {
                order.lock().unwrap().push("finish");
            }
        };
        let result = crate::test_support::with_gtk(move || {
            new_saver_with_finish(Rc::new(saving), Rc::new(finishing));
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                saver.mark_dirty()();
                saver.flush()
            })
        });
        assert!(matches!(result, Some(Ok(()))));
        assert_eq!(order.lock().unwrap().as_slice(), ["save", "finish"]);
    }

    #[test]
    fn a_failed_close_save_does_not_finish() {
        if !gtk_available() {
            return;
        }
        let _serial = SERIAL.lock().unwrap();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counting = {
            let finished = std::sync::Arc::clone(&finished);
            move || {
                finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let result = crate::test_support::with_gtk(move || {
            new_saver_with_finish(
                Rc::new(|| AutoSaveResult::Failed(anyhow::anyhow!("fixture failure"))),
                Rc::new(counting),
            );
            SAVER.with(|slot| {
                let saver = slot.borrow();
                let saver = saver.as_ref().unwrap();
                saver.mark_dirty()();
                saver.flush()
            })
        });
        assert!(matches!(result, Some(Err(_))));
        assert_eq!(finished.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
