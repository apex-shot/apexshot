//! Shared helpers for the unit tests that need a live GTK.
//!
//! GTK may be initialized only once per process, on exactly one thread:
//! gtk4-rs panics with "Attempted to initialize GTK from two different
//! threads." when a second thread calls `gtk4::init()`, and GTK objects may
//! only be touched from the thread that won. The test harness gives every test
//! its own thread, so tests that each called `gtk4::init()` fought over that
//! single slot and failed depending on scheduling order (CI: 1086 passed,
//! 1 failed in `gtk_fast_path_is_never_taken_off_the_main_thread`).
//!
//! [`with_gtk`] owns the one permitted GTK thread for the whole test binary.
//! Route every GTK call through it and initialization stays deterministic no
//! matter how the harness orders or parallelizes tests.

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// The test binary's GTK thread, or `Err(())` when GTK cannot be used in this
/// process (no display server, or another thread already owns GTK).
static GTK_THREAD: OnceLock<Result<Sender<Job>, ()>> = OnceLock::new();

/// Run `f` on the test binary's GTK thread and hand its result back.
///
/// Returns `None` when GTK is unavailable, so the caller can skip its GTK
/// assertions instead of failing. Panics raised inside `f` are re-raised in the
/// calling test, so an assertion failure is still reported against the test
/// that made it.
///
/// Do not call this from inside a [`with_gtk`] job: the GTK thread would wait
/// on itself.
pub(crate) fn with_gtk<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    let jobs = match GTK_THREAD.get_or_init(spawn_gtk_thread) {
        Ok(jobs) => jobs,
        Err(()) => return None,
    };

    let (done_tx, done_rx) = channel();
    let job: Job = Box::new(move || {
        let _ = done_tx.send(catch_unwind(AssertUnwindSafe(f)));
    });
    jobs.send(job).ok()?;

    match done_rx.recv() {
        Ok(Ok(value)) => Some(value),
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => None,
    }
}

pub(crate) fn test_app() -> gtk4::Application {
    use gtk4::prelude::ApplicationExt;
    // Its own application id: this app lives for the whole test run on the
    // shared GTK thread, so reusing the production id would collide with
    // tests that register a short-lived app under that id ("An object is
    // already exported for the interface org.gtk.Application").
    thread_local! {
        static TEST_APP: gtk4::Application = {
            let app = gtk4::Application::builder()
                .application_id(format!("{}.Tests", crate::app_identity::app_id()))
                .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
                .build();
            app.register(None::<&gtk4::gio::Cancellable>)
                .expect("the test application to register");
            app
        };
    }
    TEST_APP.with(|app| app.clone())
}

/// Start the GTK thread and report whether GTK initialized on it.
fn spawn_gtk_thread() -> Result<Sender<Job>, ()> {
    let (ready_tx, ready_rx) = channel::<bool>();
    let (job_tx, job_rx) = channel::<Job>();

    std::thread::Builder::new()
        .name("gtk-test-main".to_string())
        .spawn(move || {
            let available = if gtk4::is_initialized() {
                // Some other thread got there first. gtk4-rs forbids a second
                // `gtk4::init()`, and GTK objects cannot be used from a thread
                // that did not initialize it, so the GUI checks have to skip.
                eprintln!(
                    "test_support: GTK is already initialized on another thread; \
                     skipping GTK-dependent assertions"
                );
                false
            } else {
                gtk4::init().is_ok()
            };
            if ready_tx.send(available).is_err() || !available {
                return;
            }
            // GTK owns the default main context on this thread once it
            // initializes; pump it between jobs so default-context sources
            // (idle draws, timeouts) still dispatch while tests run.
            loop {
                while gtk4::glib::MainContext::default().iteration(false) {}
                match job_rx.try_recv() {
                    Ok(job) => job(),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .map_err(|_| ())?;

    match ready_rx.recv() {
        Ok(true) => Ok(job_tx),
        _ => Err(()),
    }
}
