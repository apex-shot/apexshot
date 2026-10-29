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

pub mod notify {
    /// The Windows build has no notification transport yet; keep the call
    /// visible in logs instead of pretending a toast appeared.
    pub fn desktop_notification(summary: &str, body: &str) {
        eprintln!("[notify] {summary}: {body}");
    }

    /// The shared cloud client calls the important variant after a successful
    /// device login. It is still only a log line on Windows.
    pub fn desktop_notification_important(summary: &str, body: &str) {
        eprintln!("[notify] {summary}: {body}");
    }
}

pub mod open {
    use std::path::Path;

    /// Open a URL with the default Windows handler.
    pub fn open_url(url: &str) -> anyhow::Result<()> {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
        Ok(())
    }

    /// Show `path` in Explorer with the file itself selected. Explorer reports
    /// a failure exit code even when it succeeds, so only the spawn counts.
    pub fn reveal_in_file_manager(path: &Path) -> anyhow::Result<()> {
        std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(path)
            .spawn()?;
        Ok(())
    }
}

#[path = "../../../src/utils/png_io.rs"]
pub mod png_io;
