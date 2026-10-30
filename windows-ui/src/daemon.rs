//! Daemon hooks used by the shared shortcut editor.
//!
//! Coordinate shortcut capture with the Windows shell's hotkey listener.

#[cfg(target_os = "windows")]
pub fn set_daemon_hotkey_suppressed(suppressed: bool) -> anyhow::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_APP,
    };

    let class: Vec<u16> = "ApexShotGlobalHotkeys"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let window = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if window.is_null() {
        return Ok(());
    }
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            WM_APP + 0x531,
            usize::from(suppressed),
            std::process::id() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            &mut result,
        )
    };
    anyhow::ensure!(
        sent != 0 && result == 1,
        "could not pause the shortcut listener"
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_daemon_hotkey_suppressed(_suppressed: bool) -> anyhow::Result<()> {
    Ok(())
}
