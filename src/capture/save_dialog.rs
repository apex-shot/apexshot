//! Shared screenshot "Save" chooser.
//!
//! Quick Access, the image editor and the "Save last screenshot" action all ask
//! for a destination the same way: a native Save dialog pre-named from the
//! configured format, writing the capture exactly where the user pointed it and
//! reporting failures. The caller owns the returned dialog and must keep it
//! alive until the completion callback runs.

use gtk4::{prelude::*, FileChooserAction, FileChooserNative, ResponseType, Window};
use std::path::{Path, PathBuf};

use super::{generate_filename, save_image_to_path, ImageFormat, SaveConfig};

/// File extensions this action accepts as screenshot data.
const SCREENSHOT_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// True when `path` holds something the Save chooser may write out again:
/// a capture in one of the image formats ApexShot produces.
pub fn is_supported_screenshot(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| SCREENSHOT_EXTENSIONS.contains(&extension.as_str()))
}

/// Encoding for the destination the picker confirmed. The chosen path is used
/// exactly as confirmed, so an overwrite prompt always covers the file that is
/// written; a recognised extension picks the encoding, otherwise the
/// configured format does.
pub fn save_as_format(destination: &Path, configured: ImageFormat) -> ImageFormat {
    let extension = destination
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());

    match extension.as_deref() {
        Some("png") => ImageFormat::Png,
        Some("jpg") | Some("jpeg") => match configured {
            ImageFormat::Jpeg { quality } => ImageFormat::Jpeg { quality },
            _ => ImageFormat::Jpeg { quality: 85 },
        },
        Some("webp") => ImageFormat::WebP,
        _ => configured,
    }
}

/// Destination a finished Save dialog selected, or `None` when the user
/// cancelled (including a response that carries no file).
pub fn save_outcome(response: ResponseType, chosen: Option<PathBuf>) -> Option<PathBuf> {
    if response != ResponseType::Accept {
        return None;
    }
    chosen
}

/// Where the Save dialog opens: the configured screenshot export location,
/// else the user's Pictures folder.
pub fn save_dialog_folder(config: &crate::config::AppConfig) -> Option<PathBuf> {
    let configured = SaveConfig::default()
        .with_output_dir(&config.screenshot_export_location)
        .get_output_dir()
        .ok()
        .filter(|dir| dir.is_dir());

    configured.or_else(|| dirs::picture_dir().filter(|dir| dir.is_dir()))
}

/// Show the Save chooser for `source`.
///
/// `completion` receives `Some(destination)` only once the capture was written
/// there; cancellation and write failures report `None`. The returned dialog is
/// still live, so the caller has to hold on to it until `completion` runs.
pub fn show_save_dialog(
    parent: Option<&Window>,
    source: PathBuf,
    completion: impl Fn(Option<PathBuf>) + 'static,
) -> FileChooserNative {
    let config = crate::config::load_config().sanitized();
    let configured_format = ImageFormat::from_setting(&config.screenshot_format);

    let chooser = FileChooserNative::new(
        Some(&crate::i18n::t("Select screenshot save location")),
        parent,
        FileChooserAction::Save,
        Some(&crate::i18n::t("Save")),
        Some(&crate::i18n::t("Cancel")),
    );
    chooser.set_current_name(&generate_filename(
        &SaveConfig::default().with_format(configured_format),
    ));
    if let Some(folder) = save_dialog_folder(&config) {
        let _ = chooser.set_current_folder(Some(&gtk4::gio::File::for_path(folder)));
    }

    chooser.connect_response(move |chooser, response| {
        chooser.hide();

        let destination = save_outcome(response, chooser.file().and_then(|file| file.path()))
            .and_then(|destination| {
                let format = save_as_format(&destination, configured_format);
                match save_image_to_path(&source, &destination, format) {
                    Ok(()) => {
                        eprintln!(
                            "[save] Wrote {} to {}",
                            source.display(),
                            destination.display()
                        );
                        Some(destination)
                    }
                    Err(error) => {
                        eprintln!("[save] Failed to write {}: {error}", destination.display());
                        let detail = format!("{}: {error}", destination.display());
                        crate::utils::notify::desktop_notification(
                            &crate::i18n::t("Screenshot not saved"),
                            &crate::i18n::tfmt("Save failed: {message}", &[("message", &detail)]),
                        );
                        None
                    }
                }
            });

        completion(destination);
    });

    chooser.show();
    chooser
}

/// Entry point for the `save-capture-internal` command: a GTK-only process that
/// asks for a destination for `path` and writes it there.
pub fn run_save_capture_command(path: PathBuf) -> Result<(), String> {
    if let Err(error) = gtk4::init() {
        return Err(format!("GTK initialization failed: {error}"));
    }
    crate::i18n::init_from_config();

    if !is_supported_screenshot(&path) {
        return Err(format!(
            "{} is not a supported screenshot file",
            path.display()
        ));
    }
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }

    let main_loop = gtk4::glib::MainLoop::new(None, false);
    let loop_quit = main_loop.clone();
    let _chooser = show_save_dialog(None, path, move |_destination| loop_quit.quit());
    main_loop.run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_cancel_leaves_the_capture_where_it_is() {
        assert_eq!(
            save_outcome(ResponseType::Cancel, Some("/tmp/picked.png".into())),
            None
        );
        assert_eq!(save_outcome(ResponseType::DeleteEvent, None), None);
        assert_eq!(save_outcome(ResponseType::Accept, None), None);
    }

    #[test]
    fn save_keeps_the_confirmed_destination_exactly() {
        assert_eq!(
            save_outcome(ResponseType::Accept, Some(PathBuf::from("/tmp/picked.png"))),
            Some(PathBuf::from("/tmp/picked.png"))
        );
        assert_eq!(
            save_outcome(ResponseType::Accept, Some(PathBuf::from("/tmp/picked"))),
            Some(PathBuf::from("/tmp/picked"))
        );
        assert_eq!(
            save_outcome(
                ResponseType::Accept,
                Some(PathBuf::from("/tmp/picked.tiff"))
            ),
            Some(PathBuf::from("/tmp/picked.tiff"))
        );
    }

    #[test]
    fn format_follows_a_recognised_extension_otherwise_the_configuration() {
        assert_eq!(
            save_as_format(Path::new("/tmp/picked.png"), ImageFormat::WebP),
            ImageFormat::Png
        );
        assert_eq!(
            save_as_format(Path::new("/tmp/picked.jpeg"), ImageFormat::Png),
            ImageFormat::Jpeg { quality: 85 }
        );
        assert_eq!(
            save_as_format(Path::new("/tmp/picked.webp"), ImageFormat::Png),
            ImageFormat::WebP
        );
        assert_eq!(
            save_as_format(Path::new("/tmp/picked"), ImageFormat::WebP),
            ImageFormat::WebP
        );
        assert_eq!(
            save_as_format(Path::new("/tmp/picked.tiff"), ImageFormat::Png),
            ImageFormat::Png
        );
    }

    #[test]
    fn only_screenshot_extensions_are_saveable() {
        assert!(is_supported_screenshot(Path::new("/tmp/shot.png")));
        assert!(is_supported_screenshot(Path::new("/tmp/shot.JPG")));
        assert!(is_supported_screenshot(Path::new("/tmp/shot.webp")));
        assert!(!is_supported_screenshot(Path::new("/tmp/clip.mp4")));
        assert!(!is_supported_screenshot(Path::new("/tmp/shot")));
    }
}
