//! Regression test for the second-recording portal hang.
//!
//! Observed against xdg-desktop-portal on GNOME 50: after a portal
//! `Session.Close`, the zbus proxy machinery of the connection that sent the
//! Close wedges — every later portal call on that connection hangs without
//! even reaching the bus. A process-global portal connection therefore broke
//! every recording after the first.
//!
//! The recording path now gives each session its own connection
//! (`Screencast::with_connection`). This test mirrors that pattern with
//! dialog-free calls (`CreateSession` never shows UI; `SelectSources`/`Start`
//! are never called) and asserts sequential sessions keep working:
//!
//!   take-1: fresh connection → create → close
//!   take-2: another fresh connection → create → close
//!
//! If take-2 times out, the per-recording-connection guarantee regressed.

use ashpd::desktop::{screencast::Screencast, CreateSessionOptions, Session};
use std::time::Duration;

const STEP_TIMEOUT: Duration = Duration::from_secs(10);

fn create_session_on_fresh_connection(step: &str) -> Result<String, String> {
    let step = step.to_string();
    let panic_step = step.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(async move {
            let conn = tokio::time::timeout(STEP_TIMEOUT, zbus::Connection::session())
                .await
                .map_err(|_| format!("{step}: session bus connect timed out"))?
                .map_err(|e| format!("{step}: session bus connect: {e}"))?;

            let proxy =
                tokio::time::timeout(STEP_TIMEOUT, Screencast::with_connection(conn.clone()))
                    .await
                    .map_err(|_| format!("{step}: Screencast::with_connection timed out"))?
                    .map_err(|e| format!("{step}: Screencast::with_connection: {e}"))?;

            let session: Session<Screencast> = tokio::time::timeout(
                STEP_TIMEOUT,
                proxy.create_session(CreateSessionOptions::default()),
            )
            .await
            .map_err(|_| format!("{step}: create_session timed out"))?
            .map_err(|e| format!("{step}: create_session: {e}"))?;

            let path = format!("{session:?}");

            match tokio::time::timeout(Duration::from_secs(2), session.close()).await {
                Ok(Ok(())) => Ok(format!("{path} (closed ok)")),
                Ok(Err(e)) => Ok(format!("{path} (close error: {e})")),
                Err(_) => Err(format!("{path} (close timed out)")),
            }
        })
    })
    .join()
    .map_err(move |_| format!("{panic_step}: worker thread panicked"))?
}

#[test]
fn sequential_portal_sessions_on_fresh_connections_do_not_hang() {
    let take1 = create_session_on_fresh_connection("take-1");
    println!("take-1: {take1:?}");
    assert!(take1.is_ok(), "first portal session must work: {take1:?}");

    let take2 = create_session_on_fresh_connection("take-2");
    println!("take-2: {take2:?}");
    assert!(
        take2.is_ok(),
        "second recording must not hang after the first session closed: {take2:?}"
    );

    let take3 = create_session_on_fresh_connection("take-3");
    println!("take-3: {take3:?}");
    assert!(take3.is_ok(), "third portal session must work: {take3:?}");
}
