//! Shared clipboard utilities for Wayland and X11.
//!
//! Provides consistent clipboard operations across all capture modes
//! (area, fullscreen, crosshair, OCR, preview overlay, editor).

use std::io::Write;
use std::path::Path;

use gtk4::glib::prelude::ToValue;
use gtk4::prelude::DisplayExt;

/// True only on the thread running the GTK main loop. GDK is not thread-safe:
/// calling clipboard APIs from a worker thread (e.g. the daemon's capture or
/// upload threads) can silently no-op while reporting success, which then
/// masks the `wl-copy`/`xclip` fallback and nothing lands on the clipboard.
fn on_gtk_main_thread() -> bool {
    gtk4::glib::MainContext::default().is_owner()
}

/// Put `provider` on GDK's own clipboard, on the GTK main thread.
///
/// This is the native path for GUI copies — what file managers, screenshot
/// tools, and the GNOME tutorial do. The running app owns the selection
/// in-process: no helper process or stray window, and GDK converts to
/// whichever MIME type the paste target asks for (image/png, image/bmp,
/// text/uri-list, …) instead of offering a single frozen format.
///
/// Returns false when there is no display, when called off the main thread, or
/// when GDK refuses the content, so callers fall back to external helpers.
fn gtk_clipboard_set_provider(provider: &gtk4::gdk::ContentProvider) -> bool {
    if !on_gtk_main_thread() {
        return false;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return false;
    };
    let clipboard = display.clipboard();
    if clipboard.set_content(Some(provider)).is_err() {
        return false;
    }
    store_clipboard_for_exit(&clipboard);
    true
}

/// Put `text` on GDK's own clipboard, on the GTK main thread.
fn gtk_clipboard_set_text(text: &str) -> bool {
    if !on_gtk_main_thread() {
        return false;
    }
    let Some(display) = gtk4::gdk::Display::default() else {
        return false;
    };
    let clipboard = display.clipboard();
    clipboard.set_text(text);
    store_clipboard_for_exit(&clipboard);
    true
}

/// Ask the compositor's clipboard manager to take ownership so the content
/// survives this process exiting (a no-op where none is running).
fn store_clipboard_for_exit(clipboard: &gtk4::gdk::Clipboard) {
    clipboard.store_async(
        gtk4::glib::Priority::DEFAULT,
        gtk4::gio::Cancellable::NONE,
        |_| {},
    );
}

/// `text/uri-list` (+ `GdkFileList`) provider so file managers and any app
/// that accepts dropped files can paste the capture as a file reference.
fn gdk_file_provider(path: &Path) -> gtk4::gdk::ContentProvider {
    let file = gtk4::gio::File::for_path(path);
    let list = gtk4::gdk::FileList::from_array(&[file]);
    gtk4::gdk::ContentProvider::for_value(&list.to_value())
}

/// Pipe bytes to `wl-copy` (Wayland). The helper daemonizes to serve paste
/// requests, so waiting for the parent to exit is safe and the content
/// survives after our process exits. Its stdio is detached so the daemonized
/// grandchild does not keep our stdout/stderr pipes open for callers that
/// capture output (e.g. tests, subprocess pipelines).
fn pipe_to_wl_copy(mime_type: &str, data: &[u8]) -> Result<(), String> {
    let mut child = std::process::Command::new("wl-copy")
        .arg("--type")
        .arg(mime_type)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Clipboard tool not found (install wl-clipboard)".to_string()
            } else {
                format!("Clipboard command failed: {e}")
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .map_err(|e| format!("Clipboard command failed: {e}"))?;
    }

    if child
        .wait()
        .map_err(|e| format!("Clipboard command failed: {e}"))?
        .success()
    {
        Ok(())
    } else {
        Err("Clipboard command failed".to_string())
    }
}

/// Pipe bytes to `xclip` (X11). Unlike `wl-copy`, `xclip` stays running to
/// serve the selection, so we must NOT `wait()` on it — that would hang the
/// caller forever. Spawn success + full stdin write means the copy landed.
fn pipe_to_xclip(args: &[&str], data: &[u8]) -> Result<(), String> {
    let mut child = std::process::Command::new("xclip")
        .args(args)
        .stdin(std::process::Stdio::piped())
        // Detach: the child outlives us to serve paste requests.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Clipboard tool not found (install xclip)".to_string()
            } else {
                format!("Clipboard command failed: {e}")
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .map_err(|e| format!("Clipboard command failed: {e}"))?;
    }
    // Intentionally no `child.wait()` here: xclip keeps running to own the
    // selection, so waiting would hang the caller. Reap in the background to
    // avoid zombies once xclip exits (e.g. selection replaced).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Offer bytes on the system clipboard via external helpers.
///
/// `xclip` is preferred whenever `DISPLAY` exists, even on Wayland sessions:
/// XWayland's selection bridge needs no input focus and its window is unmapped,
/// whereas on GNOME Wayland `wl-copy` maps a focus-seeking surface that blocks
/// the caller until it is focused (a stray window plus a frozen UI). Helpers
/// keep serving after the spawning process exits, unlike in-process handles.
fn copy_bytes_via_external(
    mime_type: &str,
    data: &[u8],
    xclip_args: &[&str],
) -> Result<(), String> {
    let has_display = std::env::var_os("DISPLAY").is_some();

    let xclip_err = if has_display {
        match pipe_to_xclip(xclip_args, data) {
            Ok(()) => return Ok(()),
            Err(e) => Some(e),
        }
    } else {
        None
    };

    match pipe_to_wl_copy(mime_type, data) {
        Ok(()) => Ok(()),
        Err(wl_err) => {
            // Surface the error from the session we're actually in.
            if has_display {
                Err(xclip_err.unwrap_or(wl_err))
            } else {
                Err(wl_err)
            }
        }
    }
}

/// Copy a file URI to the clipboard as `text/uri-list`.
///
/// GUI callers (GTK main thread) use GDK's in-process clipboard; background
/// callers fall back to `xclip`/`wl-copy`. Portal-only copies plain path text.
pub fn copy_uri_to_clipboard(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| "Failed to convert path to file URI".to_string())?;

    if crate::app_identity::portal_only() {
        return copy_text_to_clipboard(&uri);
    }

    if gtk_clipboard_set_provider(&gdk_file_provider(path)) {
        return Ok(());
    }

    let payload = format!("{uri}\r\n");
    copy_bytes_via_external(
        "text/uri-list",
        payload.as_bytes(),
        &["-selection", "clipboard", "-t", "text/uri-list", "-i"],
    )
}

/// Copy text to the system clipboard.
///
/// GUI callers (GTK main thread) use GDK's in-process clipboard; background
/// callers fall back to `xclip`/`wl-copy` and then arboard.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    if gtk_clipboard_set_text(text) {
        return Ok(());
    }

    let mime_type = "text/plain;charset=utf-8";
    if copy_bytes_via_external(
        mime_type,
        text.as_bytes(),
        &["-selection", "clipboard", "-i"],
    )
    .is_ok()
    {
        return Ok(());
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {e}"))?;

    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard text: {e}"))?;

    Ok(())
}

/// Copy text through GTK's clipboard on its main thread.
///
/// This is the non-command path used by Quick Access and both editors so a
/// successful upload never launches a visible `wl-copy` helper notification.
pub fn copy_text_to_gtk_clipboard(text: &str) -> Result<(), String> {
    let display = gtk4::gdk::Display::default()
        .ok_or_else(|| "No GTK display is available for clipboard access".to_string())?;
    display.clipboard().set_text(text);
    Ok(())
}

/// Normalize image bytes to PNG: screenshots may be saved as JPEG/WebP, but
/// the clipboard offer is `image/png` — pasting mislabeled bytes shows a
/// broken image. PNG input passes through untouched.
fn normalize_to_png_bytes(image_data: &[u8]) -> Vec<u8> {
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
    if image_data.starts_with(PNG_MAGIC) {
        return image_data.to_vec();
    }
    let Ok(img) = image::load_from_memory(image_data) else {
        return image_data.to_vec();
    };
    let mut png = Vec::new();
    if img
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .is_ok()
    {
        png
    } else {
        image_data.to_vec()
    }
}

/// True when the in-process GDK clipboard is usable: on the GTK main thread
/// with a display, outside Flatpak's portal-only path.
pub fn gdk_clipboard_available() -> bool {
    !crate::app_identity::portal_only()
        && on_gtk_main_thread()
        && gtk4::gdk::Display::default().is_some()
}

/// Copy an image file to the clipboard as a PNG image.
///
/// GUI callers get an image + file-reference union on GDK's clipboard, so both
/// image editors and file managers can paste it. Background callers use
/// `xclip`/`wl-copy`; portal-only uses in-process arboard.
pub fn copy_image_to_clipboard(path: &Path) -> Result<(), String> {
    copy_image_to_clipboard_inner(path, true)
}

/// Copy an image file as `image/png` only, with no file reference attached so
/// paste targets cannot pick the URI instead of the bitmap.
pub fn copy_image_only_to_clipboard(path: &Path) -> Result<(), String> {
    copy_image_to_clipboard_inner(path, false)
}

fn copy_image_to_clipboard_inner(path: &Path, include_file_reference: bool) -> Result<(), String> {
    let image_data = std::fs::read(path).map_err(|e| format!("Failed to read image file: {e}"))?;

    if crate::app_identity::portal_only() {
        return copy_image_bytes_via_arboard(&image_data);
    }

    let png_data = normalize_to_png_bytes(&image_data);

    if gdk_clipboard_available() {
        let image_provider =
            gtk4::gdk::ContentProvider::for_bytes("image/png", &gtk4::glib::Bytes::from(&png_data));
        let provider = if include_file_reference {
            // Union with the file reference: pasting into an image editor yields
            // the bitmap, pasting into a file manager yields the file.
            gtk4::gdk::ContentProvider::new_union(&[image_provider, gdk_file_provider(path)])
        } else {
            image_provider
        };
        if gtk_clipboard_set_provider(&provider) {
            return Ok(());
        }
    }

    copy_bytes_via_external(
        "image/png",
        &png_data,
        &["-selection", "clipboard", "-t", "image/png", "-i"],
    )
}

/// Settings → Screenshots → "Clipboard copy behavior".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotClipboardMode {
    ImageOnly,
    FilePathOnly,
    Both,
}

impl ScreenshotClipboardMode {
    pub fn from_config_value(value: &str) -> Self {
        match value {
            "Image Only" => Self::ImageOnly,
            "File Path Only" => Self::FilePathOnly,
            _ => Self::Both,
        }
    }

    /// True when the mode puts the bitmap on the clipboard.
    pub fn includes_image(self) -> bool {
        matches!(self, Self::ImageOnly | Self::Both)
    }
}

/// Copy a screenshot according to the configured clipboard mode.
///
/// "File & Image" offers both formats when GDK owns the selection in-process
/// (the image provider is unioned with the file list). `xclip`/`wl-copy` can
/// only own one format each, so there the file URI is copied last and owns the
/// selection, matching the daemon's historical behavior.
pub fn copy_screenshot_with_mode(path: &Path, mode: ScreenshotClipboardMode) -> Result<(), String> {
    match mode {
        ScreenshotClipboardMode::ImageOnly => copy_image_only_to_clipboard(path),
        ScreenshotClipboardMode::FilePathOnly => copy_uri_to_clipboard(path),
        ScreenshotClipboardMode::Both => {
            if gdk_clipboard_available() {
                // GDK's image provider already unions in the file reference.
                copy_image_to_clipboard(path)
            } else {
                let image_result = copy_image_only_to_clipboard(path);
                let uri_result = copy_uri_to_clipboard(path);
                image_result.and(uri_result)
            }
        }
    }
}

fn copy_image_bytes_via_arboard(image_data: &[u8]) -> Result<(), String> {
    let img = image::load_from_memory(image_data)
        .map_err(|e| format!("Failed to decode image for clipboard: {e}"))?
        .to_rgba8();
    let (width, height) = img.dimensions();
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {e}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: img.into_raw().into(),
        })
        .map_err(|e| format!("Failed to set clipboard image: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_bytes_pass_through_untouched() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 32]);
        assert_eq!(normalize_to_png_bytes(&png), png);
    }

    #[test]
    fn jpeg_bytes_are_reencoded_as_png() {
        // 1x1 red JPEG built in-memory (no fixture files).
        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]))
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let out = normalize_to_png_bytes(&jpeg);
        assert!(out.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(image::load_from_memory(&out).is_ok());
    }

    #[test]
    fn screenshot_clipboard_mode_maps_settings_values() {
        use ScreenshotClipboardMode::{Both, FilePathOnly, ImageOnly};
        for (value, expected) in [
            ("Image Only", ImageOnly),
            ("File Path Only", FilePathOnly),
            ("File & Image (default)", Both),
            ("unexpected", Both),
        ] {
            assert_eq!(
                ScreenshotClipboardMode::from_config_value(value),
                expected,
                "config value {value:?}"
            );
        }
        assert!(ImageOnly.includes_image() && Both.includes_image());
        assert!(!FilePathOnly.includes_image());
    }

    #[test]
    fn gtk_fast_path_is_never_taken_off_the_main_thread() {
        // The daemon initializes GTK (so a Display exists process-wide) but
        // copies from worker threads, where GDK calls silently no-op and mask
        // the wl-copy/xclip fallback. Reproduce that shape here: even with a
        // display present, a spawned thread must not take the GTK path.
        let _ = gtk4::init();
        let display_present = gtk4::gdk::Display::default().is_some();
        let took_gtk_path = std::thread::spawn(|| gtk_clipboard_set_text("x"))
            .join()
            .unwrap();
        assert!(
            !took_gtk_path,
            "worker thread took GTK clipboard path (display present: {display_present})"
        );
    }

    #[test]
    fn external_helper_round_trips_text_and_image() {
        // History copies from the GTK main thread; this asserts the external
        // helper path actually lands on the clipboard. Text and image live in
        // one test because the clipboard is global: parallel tests would
        // clobber each other's selection.
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let payload = "apexshot-clipboard-roundtrip";
        copy_text_to_clipboard(payload).expect("copy text");
        let out = std::process::Command::new("wl-paste")
            .args(["--type", "text/plain;charset=utf-8"])
            .output()
            .expect("wl-paste");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            payload,
            "clipboard text mismatch"
        );

        // The user-facing regression: copying an image from the History UI must
        // land actual PNG bytes on the clipboard, not an empty selection.
        let path = std::env::temp_dir().join(format!(
            "apexshot-clipboard-image-{}.png",
            std::process::id()
        ));
        let source = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        source
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, &png).unwrap();

        let result = copy_image_to_clipboard(&path);
        let _ = std::fs::remove_file(&path);
        result.expect("copy image");

        let out = std::process::Command::new("wl-paste")
            .args(["--type", "image/png"])
            .output()
            .expect("wl-paste");
        assert_eq!(out.stdout, png, "clipboard image bytes mismatch");
    }
}
