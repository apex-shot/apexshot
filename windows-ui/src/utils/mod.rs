//! Shared helpers the settings pages import from `crate::utils`.

pub mod clipboard {
    use gtk4::prelude::*;

    /// Copy text without a GTK display, for the short-lived upload helper.
    ///
    /// The helper exits as soon as the upload finishes, so it must not depend
    /// on a GTK main loop that never runs.
    #[cfg(target_os = "windows")]
    pub fn copy_text_via_windows_clipboard(text: &str) -> Result<(), String> {
        use windows_sys::Win32::Foundation::GlobalFree;
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
        };
        use windows_sys::Win32::System::Memory::{
            GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
        };

        /// `CF_UNICODETEXT`, the clipboard format this text is written as.
        const CF_UNICODETEXT: u32 = 13;

        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * std::mem::size_of::<u16>();
        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return Err("the clipboard is owned by another app".to_string());
            }
            let _ = EmptyClipboard();
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if handle.is_null() {
                let _ = CloseClipboard();
                return Err("could not allocate clipboard memory".to_string());
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                let _ = GlobalFree(handle);
                let _ = CloseClipboard();
                return Err("could not lock clipboard memory".to_string());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer as *mut u16, wide.len());
            let _ = GlobalUnlock(handle);
            if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
                let _ = GlobalFree(handle);
                let _ = CloseClipboard();
                return Err("could not put the text on the clipboard".to_string());
            }
            let _ = CloseClipboard();
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn copy_text_via_windows_clipboard(_text: &str) -> Result<(), String> {
        Err("the Windows clipboard helper is only available on Windows".to_string())
    }

    pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
        let Some(display) = gtk4::gdk::Display::default() else {
            return Err("no display".to_string());
        };
        display.clipboard().set_text(text);
        Ok(())
    }

    /// The name the shared onboarding helpers call.
    pub fn copy_text_to_gtk_clipboard(text: &str) -> Result<(), String> {
        copy_text_to_clipboard(text)
    }
}

pub mod notify;

/// A command for a console program started from this GUI app.
///
/// Windows gives every child process of a windowed program a console of its
/// own unless it is told not to, which is why launching Settings used to flash
/// terminal windows before the app appeared. Every console tool this app runs
/// goes through here.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

pub mod open {
    use std::path::Path;

    /// Open a URL with the default Windows handler.
    pub fn open_url(url: &str) -> Result<(), String> {
        crate::utils::hidden_command("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// Show `path` in Explorer with the file itself selected. Explorer reports
    /// a failure exit code even when it succeeds, so only the spawn counts.
    pub fn reveal_in_file_manager(path: &Path) -> anyhow::Result<()> {
        crate::utils::hidden_command("explorer.exe")
            .arg("/select,")
            .arg(path)
            .spawn()?;
        Ok(())
    }
}

#[path = "../../../src/utils/png_io.rs"]
pub mod png_io;
