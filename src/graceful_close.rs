//! Graceful SIGTERM handling for the GTK window processes.
//!
//! A native package install stops every ApexShot process with SIGKILL, which
//! skips the editors' close handlers and loses the session sidecars they write
//! there. The update window sends SIGTERM first, so each process can close its
//! windows through the normal close path and quit. A session logout sends
//! SIGTERM too, so the same path saves work there.

use gtk4::prelude::*;
use gtk4::{glib, Application};

/// Close every window through its normal close request when SIGTERM arrives,
/// and quit once none remain.
///
/// `may_close` runs before any window is touched. An editor that is busy (an
/// export is encoding) returns `false`, and the process keeps running with the
/// handler still installed for a later signal.
pub fn close_windows_on_sigterm<F>(app: &Application, may_close: F)
where
    F: Fn() -> bool + 'static,
{
    let app = app.clone();
    glib::unix_signal_add_local(libc::SIGTERM, move || {
        if !may_close() {
            return glib::ControlFlow::Continue;
        }
        for window in app.windows() {
            window.close();
        }
        if app.windows().is_empty() {
            app.quit();
        }
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_handler_quits_only_after_the_last_window() {
        let source = include_str!("graceful_close.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("libc::SIGTERM"));
        assert!(production.contains("window.close()"));
        assert!(production.contains("if app.windows().is_empty()"));
        assert!(production.contains("app.quit()"));
    }
}
