//! File and clipboard actions for Windows History cards.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use gtk4::prelude::*;

use super::scan::{CaptureEntry, MediaKind};
use crate::i18n::{t, tfmt};

pub fn open_in_default_app(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    shell_open(&entry.path)?;
    Ok(tfmt("Opened {name}", &[("name", &entry.display_name)]))
}

pub fn open_in_apexshot_editor(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    Err(t(
        "The ApexShot editor is not available in the Windows build yet",
    ))
}

pub fn copy_to_clipboard(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;

    let Some(display) = gtk4::gdk::Display::default() else {
        return Err(t("Could not access the Windows clipboard"));
    };
    let clipboard = display.clipboard();

    match entry.kind {
        MediaKind::Image => {
            let texture = gtk4::gdk::Texture::from_filename(&entry.path).map_err(|error| {
                tfmt(
                    "Could not copy image: {message}",
                    &[("message", &error.to_string())],
                )
            })?;
            clipboard.set_texture(&texture);
            Ok(t("Image copied to clipboard"))
        }
        MediaKind::Video => {
            let list = gtk4::gdk::FileList::from_array(&[gtk4::gio::File::for_path(&entry.path)]);
            clipboard
                .set_content(Some(&gtk4::gdk::ContentProvider::for_value(
                    &list.to_value(),
                )))
                .map_err(|error| {
                    tfmt(
                        "Could not copy file: {message}",
                        &[("message", &error.to_string())],
                    )
                })?;
            Ok(t("File copied to clipboard"))
        }
    }
}

pub fn copy_link_to_clipboard(link: &str) -> Result<String, String> {
    let link = link.trim();
    if link.is_empty() {
        return Err(t("This upload has no share link"));
    }
    crate::utils::clipboard::copy_text_to_clipboard(link)?;
    Ok(t("Share link copied to clipboard"))
}

pub fn open_in_browser(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(t("This upload has no link to open"));
    }
    crate::utils::open::open_url(url).map_err(|error| error.to_string())?;
    Ok(t("Opened in your browser"))
}

pub fn reveal_in_file_manager(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(&entry.path)
        .spawn()
        .map_err(|error| {
            tfmt(
                "Could not open Explorer: {message}",
                &[("message", &error.to_string())],
            )
        })?;
    Ok(tfmt(
        "Showing {name} in your files",
        &[("name", &entry.display_name)],
    ))
}

pub fn upload_to_cloud(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    Err(t(
        "Cloud uploads are not available in the Windows build yet",
    ))
}

pub fn delete_capture(entry: &CaptureEntry) -> Result<String, String> {
    match std::fs::remove_file(&entry.path) {
        Ok(()) => Ok(tfmt("Deleted {name}", &[("name", &entry.display_name)])),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(tfmt(
            "{name} was already gone",
            &[("name", &entry.display_name)],
        )),
        Err(error) => Err(tfmt(
            "Could not delete {name}: {message}",
            &[
                ("name", &entry.display_name),
                ("message", &error.to_string()),
            ],
        )),
    }
}

fn ensure_exists(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    Err(tfmt("{name} is no longer on disk", &[("name", &name)]))
}

#[cfg(target_os = "windows")]
fn shell_open(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let file: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as usize <= 32 {
        return Err(format!("Windows could not open {}", path.display()));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn shell_open(_path: &Path) -> Result<(), String> {
    Err("History actions are only wired on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: MediaKind) -> CaptureEntry {
        CaptureEntry {
            path: PathBuf::from(format!("/nonexistent/apexshot-history/{name}")),
            display_name: name.to_string(),
            modified: None,
            size_bytes: 0,
            kind,
        }
    }

    #[test]
    fn missing_files_are_reported_before_opening_them() {
        let missing = entry("gone.png", MediaKind::Image);
        let error = open_in_default_app(&missing).unwrap_err();
        assert!(error.contains("no longer on disk"));
    }

    #[test]
    fn deleting_a_missing_file_is_not_an_error() {
        assert!(delete_capture(&entry("gone.png", MediaKind::Image)).is_ok());
    }
}
