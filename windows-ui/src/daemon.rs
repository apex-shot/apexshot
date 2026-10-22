//! Daemon hooks used by the shared shortcut editor.
//!
//! The Windows shell owns its own tray and has no global hotkey listener yet.

pub fn set_daemon_hotkey_suppressed(_suppressed: bool) -> anyhow::Result<()> {
    Ok(())
}
