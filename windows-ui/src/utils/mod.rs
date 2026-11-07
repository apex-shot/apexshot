//! Shared helpers the settings pages import from `crate::utils`.

pub mod clipboard {
    use gtk4::prelude::*;

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
    pub fn open_url(url: &str) -> anyhow::Result<()> {
        crate::utils::hidden_command("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
        Ok(())
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
