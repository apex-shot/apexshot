// Utility functions and shared helpers
// TODO: Add config handling, logging setup, etc

pub mod clipboard;
pub mod desktop_env;
pub mod notify;
pub mod open;

/// Run `f` on a thread that is not driving a Tokio runtime.
///
/// `zbus::blocking` builds its own multi-thread runtime and panics with
/// "Cannot start a runtime from within a runtime" if called on a Tokio
/// worker. Release builds use `panic = "abort"`, so that takes down the daemon.
pub fn run_off_tokio<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(f)
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    } else {
        f()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_off_tokio_from_runtime_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let value = rt.block_on(async { super::run_off_tokio(|| 7) });
        assert_eq!(value, 7);
    }
}
