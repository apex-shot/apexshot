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
}

pub mod notify {
    /// The Windows build has no notification transport yet; keep the call
    /// visible in logs instead of pretending a toast appeared.
    pub fn desktop_notification(summary: &str, body: &str) {
        eprintln!("[notify] {summary}: {body}");
    }
}

pub mod open {
    /// Open a URL with the default Windows handler.
    pub fn open_url(url: &str) -> anyhow::Result<()> {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
        Ok(())
    }
}

#[path = "../../../src/utils/png_io.rs"]
pub mod png_io;
