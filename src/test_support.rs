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
//!
//! # The private display
//!
//! Initializing GTK against the developer's session made the suite open real
//! windows and popovers, reach desktop portals, and pass or fail on that
//! session's compositor (a popover's seat, a renderer's colour handling). On a
//! machine with no display at all — CI — `gtk4::init()` failed and every GTK
//! test quietly skipped instead.
//!
//! So the harness starts its own display before the first `gtk4::init()`: a
//! private broadway daemon on a free display number, and the process is pointed
//! at it with `GDK_BACKEND=broadway`, `BROADWAY_DISPLAY=:N`,
//! `GSK_RENDERER=cairo`, `GDK_DEBUG=no-portals` and `GTK_A11Y=none`. The
//! session's `WAYLAND_DISPLAY` and `DISPLAY` are removed so GTK cannot fall
//! back to them, and nothing a test draws reaches a screen: broadway's server
//! renders offscreen and is only readable over its own protocol.
//!
//! `APEXSHOT_TEST_USE_DESKTOP=1` opts out and uses the session's display as
//! before, for the occasional test that has to be looked at. A test that needs
//! the session's own clipboard or compositor belongs behind that flag.
//!
//! When no `gtk4-broadwayd`/`broadwayd` binary exists, or the daemon cannot be
//! started, GTK tests skip exactly as they did without a display, with one line
//! on stderr saying why. That is the fallback for a stripped-down machine, not
//! the expected path: CI installs `libgtk-4-bin`, which ships the daemon.
//!
//! The private display is Linux only. Off Linux (the Windows UI shares this
//! file) GTK initializes on the platform's own display, as it did before.
//!
//! ## Why mutating the environment is safe here
//!
//! `std::env::set_var` is process-wide, so the switch happens on the GTK thread
//! before `gtk4::init()` and before any test can have a live GTK. Tests that
//! read `WAYLAND_DISPLAY`/`DISPLAY` do so to decide whether the *session* has a
//! display (clipboard helpers, backend detection); with the harness active they
//! read "none", which is the truth for the test process: it has no session.
//! That is also what makes results identical on every machine. A test that
//! genuinely needs the session must use `APEXSHOT_TEST_USE_DESKTOP=1`, where the
//! harness leaves the environment untouched.

use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// The test binary's GTK thread, or `Err(())` when GTK cannot be used in this
/// process (no display server, or another thread already owns GTK).
static GTK_THREAD: OnceLock<Result<Sender<Job>, ()>> = OnceLock::new();

/// Set by the exit handler so the GTK thread stops pumping before the daemon
/// goes away; otherwise GTK reads the closed connection and prints
/// "Unable to read from broadway server: eof" after the test summary.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

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
            // The private display has to exist, and the environment has to
            // name it, before the one `gtk4::init()` this process gets.
            if !use_private_display() {
                let _ = ready_tx.send(false);
                return;
            }
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
            if !available {
                eprintln!(
                    "test_support: GTK could not initialize on the private display; \
                     skipping GTK-dependent assertions"
                );
            }
            if ready_tx.send(available).is_err() || !available {
                return;
            }
            // GTK owns the default main context on this thread once it
            // initializes; pump it between jobs so default-context sources
            // (idle draws, timeouts) still dispatch while tests run.
            loop {
                if SHUTTING_DOWN.load(Ordering::Relaxed) {
                    break;
                }
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

/// Point this process at a display no one is looking at, and report whether
/// GTK may be initialized.
fn use_private_display() -> bool {
    if std::env::var_os("APEXSHOT_TEST_USE_DESKTOP").is_some_and(|value| value == "1") {
        eprintln!(
            "test_support: APEXSHOT_TEST_USE_DESKTOP=1, running GTK tests on this \
             session's display (windows will appear)"
        );
        return true;
    }
    use_broadway_display()
}

/// Off Linux there is no broadway daemon to start (and no parent-death signal
/// to tie it to the test binary), so GTK uses the platform's own display, as
/// it always has on the Windows runners.
#[cfg(not(target_os = "linux"))]
fn use_broadway_display() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn use_broadway_display() -> bool {
    let Some(display) = broadway::start_broadwayd() else {
        eprintln!(
            "test_support: no broadway daemon could be started; skipping GTK-dependent \
             assertions (install libgtk-4-bin for gtk4-broadwayd, or set \
             APEXSHOT_TEST_USE_DESKTOP=1 to use this session's display)"
        );
        return false;
    };
    // Before `gtk4::init()`: GDK reads the backend and display from the
    // environment, and the session's own display must not be reachable as a
    // fallback. See the module docs for why mutating the process environment
    // is safe here.
    std::env::set_var("GDK_BACKEND", "broadway");
    std::env::set_var("BROADWAY_DISPLAY", format!(":{}", display));
    // Cairo is broadway's renderer and the one every machine has, so a
    // developer's `GSK_RENDERER=gl` cannot change what the tests measure.
    std::env::set_var("GSK_RENDERER", "cairo");
    // No portal round-trips for settings or the clipboard: they would reach the
    // developer's desktop services and make results depend on them.
    std::env::set_var("GDK_DEBUG", "no-portals");
    std::env::set_var("GTK_A11Y", "none");
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("DISPLAY");
    true
}

/// The private broadway daemon. Linux only: it relies on `PR_SET_PDEATHSIG`
/// so the daemon cannot outlive the test binary.
#[cfg(target_os = "linux")]
mod broadway {
    use super::SHUTTING_DOWN;
    use std::net::TcpStream;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    /// The private broadway daemon this process started, if any. Kept for its
    /// pid so the exit handler can stop it; the daemon also carries a
    /// parent-death signal, so it cannot outlive the test binary even if it is
    /// killed.
    static DAEMON_PID: AtomicI32 = AtomicI32::new(0);

    /// Start a private broadway daemon on a free display number and return it.
    pub(super) fn start_broadwayd() -> Option<u32> {
        let candidates = broadwayd_candidates();
        for binary in &candidates {
            for display in 1..=16u32 {
                // broadwayd serves display `:N` on TCP port 8080 + N.
                let port = 8080 + display as u16;
                if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    continue;
                }
                let Ok(mut child) = spawn_broadwayd(binary, display) else {
                    // Not executable, or gone: try the next candidate.
                    break;
                };
                if wait_for_broadwayd(&mut child, port) {
                    let pid = child.id() as i32;
                    DAEMON_PID.store(pid, Ordering::Relaxed);
                    // `Child` is dropped without waiting on purpose: the daemon is
                    // meant to outlive this call and is stopped by the exit handler
                    // and the parent-death signal.
                    std::mem::forget(child);
                    register_exit_handler();
                    return Some(display);
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        None
    }

    /// The broadway daemon binaries on `PATH`, in the order to try them.
    fn broadwayd_candidates() -> Vec<std::path::PathBuf> {
        let Some(path) = std::env::var_os("PATH") else {
            return Vec::new();
        };
        std::env::split_paths(&path)
            .flat_map(|dir| {
                ["gtk4-broadwayd", "broadwayd"]
                    .into_iter()
                    .map(move |name| dir.join(name))
            })
            .filter(|candidate| candidate.is_file())
            .collect()
    }

    fn spawn_broadwayd(binary: &std::path::Path, display: u32) -> std::io::Result<Child> {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(binary);
        command
            .arg(format!(":{display}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // The daemon must not outlive the test binary, including when it is killed.
        // The signal is tied to the thread that spawns it, so this runs on the GTK
        // thread, which lives as long as the binary.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn()
    }

    /// Whether the daemon is answering on `port`, or has died trying.
    fn wait_for_broadwayd(child: &mut Child, port: u16) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return true;
            }
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    extern "C" fn stop_daemon() {
        let pid = DAEMON_PID.load(Ordering::Relaxed);
        if pid > 0 {
            // Let the GTK thread leave its pump first, so it is not reading the
            // connection when the daemon closes it.
            SHUTTING_DOWN.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(20));
            // The daemon is ours; SIGTERM lets it remove its socket.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    }

    /// Register [`stop_daemon`] once, so the daemon is stopped when the test binary
    /// exits rather than lingering until the next run needs its port.
    fn register_exit_handler() {
        static REGISTERED: OnceLock<()> = OnceLock::new();
        REGISTERED.get_or_init(|| unsafe {
            libc::atexit(stop_daemon);
        });
    }
}

/// Tests that need ffmpeg or ffprobe call this where they would skip. CI sets
/// `APEXSHOT_REQUIRE_MEDIA_TESTS` so a missing tool fails the test instead of
/// letting it pass without running.
pub(crate) fn skip_without_media(reason: &str) {
    if std::env::var_os("APEXSHOT_REQUIRE_MEDIA_TESTS").is_some() {
        panic!("{reason} (APEXSHOT_REQUIRE_MEDIA_TESTS is set, so this test must run)");
    }
    eprintln!("{reason}");
}
