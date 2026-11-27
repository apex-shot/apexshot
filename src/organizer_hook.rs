//! Name and index a saved Linux capture with the shared organizer.
//!
//! Captures keep their normal pace: the window context and OCR run on a worker
//! thread, and the saved path is handed back after at most 3 seconds.
//! A capture that misses that window is still indexed, but never renamed after
//! its path went out, so every caller keeps a valid file.

use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use apexshot_organizer::organize::{
    organize_capture, parse_name_style, OrganizeRequest, OrganizeSettings,
};
use apexshot_organizer::{OcrPage, TextLine};
use image::RgbaImage;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use crate::config::AppConfig;
use crate::ocr::DetectedTextRegion;

/// How long a capture waits for its name before its saved path goes out.
const ORGANIZER_WAIT: Duration = Duration::from_millis(3000);

/// Name, rename, and index a saved capture, and return where it lives now.
///
/// The original path comes back on every failure, including a missing index
/// location, an unreadable image, or a rename that lost the file.
pub fn organize_saved_capture(path: PathBuf, config: &AppConfig) -> PathBuf {
    organize_saved_capture_gated(path, config, &|| true)
}

/// Like [`organize_saved_capture`], but asks `may_rename` at the moment the
/// worker's request arrives before deciding whether the file may be renamed.
///
/// A caller that already handed the path out (for example to a Quick Access
/// card) passes a gate that turns false once that path can no longer follow a
/// rename. The capture is still indexed, under the path it went out with.
pub fn organize_saved_capture_gated(
    path: PathBuf,
    config: &AppConfig,
    may_rename: &dyn Fn() -> bool,
) -> PathBuf {
    let Some(index_path) = crate::config::organizer_index_path() else {
        return path;
    };
    if !path.is_file() {
        return path;
    }

    let original = path.clone();
    let started = Instant::now();
    let settings = OrganizeSettings {
        auto_name: config.organizer_auto_name,
        style: parse_name_style(&config.organizer_name_style),
        excluded_apps: config.organizer_excluded_apps.clone(),
    };
    let language = config.adv_ocr_language.clone();

    // The worker sends the finished request; the capture path answers on the
    // decision channel. Dropping that sender tells the worker the path went
    // out without it, so the worker indexes on its own instead of renaming.
    let (request_tx, request_rx) = mpsc::channel();
    let (decision_tx, decision_rx) = mpsc::channel();
    let worker_settings = settings.clone();
    let worker_index = index_path.clone();
    let worker_path = path;
    let spawned = std::thread::Builder::new()
        .name("apexshot-organizer".to_string())
        .spawn(move || {
            organize_after_collection(
                request_tx,
                decision_rx,
                worker_path,
                language,
                worker_settings,
                worker_index,
                started,
            );
        });
    if let Err(error) = spawned {
        eprintln!("[organizer] worker did not start: {error}");
        return original;
    }

    let request = match request_rx.recv_timeout(ORGANIZER_WAIT) {
        Ok(request) => request,
        Err(_) => {
            // Dropping both ends tells the worker to index the file without
            // renaming it, under the path this capture returns.
            drop(decision_tx);
            drop(request_rx);
            eprintln!(
                "[organizer] {} kept its name: context and text took longer than {} ms",
                original.display(),
                ORGANIZER_WAIT.as_millis()
            );
            return original;
        }
    };
    let _ = decision_tx.send(());

    // The caller's card may have already shown this path, so ask it here, just
    // before the organizer decides on a name.
    let settings = effective_settings(settings, may_rename());
    let outcome = organize_capture(&request, &settings, &index_path, chrono::Local::now());
    let elapsed = started.elapsed().as_millis();
    if !outcome.path.exists() {
        eprintln!(
            "[organizer] {} kept its name: the organizer chose {} but that file is missing",
            original.display(),
            outcome.path.display()
        );
        return original;
    }

    if outcome.renamed && outcome.path.file_name() != original.file_name() {
        crate::studio_nudges::mark_name_hint_pending();
    }
    if outcome.renamed {
        eprintln!(
            "[organizer] {} -> {} in {} ms",
            original.display(),
            outcome.path.display(),
            elapsed
        );
    } else {
        eprintln!(
            "[organizer] {} kept its name in {} ms",
            original.display(),
            elapsed
        );
    }
    outcome.path
}

/// Keep the configured naming style, but never rename once the caller that
/// already has the path cannot follow one.
fn effective_settings(mut settings: OrganizeSettings, may_rename: bool) -> OrganizeSettings {
    if !may_rename {
        settings.auto_name = false;
    }
    settings
}

/// Collect on the worker thread, then either hand the request to the waiting
/// capture path or index without renaming once that path has gone out.
fn organize_after_collection(
    request_tx: mpsc::Sender<OrganizeRequest>,
    decision_rx: mpsc::Receiver<()>,
    path: PathBuf,
    language: String,
    mut settings: OrganizeSettings,
    index_path: PathBuf,
    started: Instant,
) {
    let request = build_request(path, &language);
    // The capture path gets its own copy; this one stays available if the
    // path already returned before the request arrived.
    if request_tx.send(request.clone()).is_ok() && decision_rx.recv().is_ok() {
        return;
    }

    // The file's path already went out, so index it under that path and never
    // rename it here.
    settings.auto_name = false;
    let outcome = organize_capture(&request, &settings, &index_path, chrono::Local::now());
    let elapsed = started.elapsed().as_millis();
    if outcome.indexed {
        eprintln!(
            "[organizer] {} indexed after the capture was handed out in {} ms",
            request.path.display(),
            elapsed
        );
    } else {
        eprintln!(
            "[organizer] {} kept its name: indexing after the capture was handed out did not finish",
            request.path.display()
        );
    }
}

/// Read the saved image and the window in front, ready for the organizer.
fn build_request(path: PathBuf, language: &str) -> OrganizeRequest {
    let (app, window_title) = capture_window_context();
    let page = read_ocr_page(&path, language);
    OrganizeRequest {
        path,
        page,
        app,
        window_title,
        // Model naming is a separate, background step and never blocks a save.
        allow_ai: false,
    }
}

/// The foreground window's app and title, best effort.
fn capture_window_context() -> (Option<String>, Option<String>) {
    let window = crate::compositor::detect_compositor()
        .and_then(|compositor| compositor.get_active_window().ok().flatten());
    if let Some(window) = window {
        return clean_window_identity(Some(window.class), Some(window.title));
    }
    if crate::backend::is_x11_session() {
        return x11_active_window_identity().unwrap_or((None, None));
    }
    (None, None)
}

/// Trim the two fields, and treat one of our own overlays as no context.
fn clean_window_identity(
    app: Option<String>,
    title: Option<String>,
) -> (Option<String>, Option<String>) {
    let app = non_empty(app);
    if app
        .as_deref()
        .is_some_and(|value| value.to_lowercase().contains("apexshot"))
    {
        return (None, None);
    }
    (app, non_empty(title))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The active window's title and class on an X11 session.
fn x11_active_window_identity() -> Option<(Option<String>, Option<String>)> {
    let (connection, screen_number) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen_number)?.root;
    let active_atom = intern_atom(&connection, b"_NET_ACTIVE_WINDOW")?;
    let reply = connection
        .get_property(false, root, active_atom, AtomEnum::ANY, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let window = reply.value32()?.next()?;
    if window == 0 {
        return None;
    }
    let title = x11_window_title(&connection, window);
    let app = x11_window_class(&connection, window);
    Some(clean_window_identity(app, title))
}

fn intern_atom(connection: &RustConnection, name: &[u8]) -> Option<u32> {
    Some(connection.intern_atom(false, name).ok()?.reply().ok()?.atom)
}

fn x11_window_title(connection: &RustConnection, window: Window) -> Option<String> {
    for name in [b"_NET_WM_NAME".as_slice(), b"WM_NAME".as_slice()] {
        let atom = intern_atom(connection, name)?;
        let reply = connection
            .get_property(false, window, atom, AtomEnum::ANY, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if reply.value.is_empty() {
            continue;
        }
        return Some(String::from_utf8_lossy(&reply.value).into_owned());
    }
    None
}

fn x11_window_class(connection: &RustConnection, window: Window) -> Option<String> {
    let reply = connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    let parts: Vec<&[u8]> = reply
        .value
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect();
    // "instance\0class\0": the class is what the window advertises to the
    // desktop, and falls back to the instance when it is all there is.
    parts
        .get(1)
        .or_else(|| parts.first())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

fn read_ocr_page(path: &Path, language: &str) -> OcrPage {
    let Ok(opened) = image::open(path) else {
        return OcrPage::default();
    };
    let image: RgbaImage = opened.to_rgba8();
    let (width, height) = (image.width(), image.height());
    match crate::ocr::extract_text_lines_for_naming(&image, language) {
        Ok(regions) => ocr_page_from_regions(width, height, regions),
        Err(error) => {
            eprintln!("[organizer] OCR failed: {error}");
            OcrPage {
                image_width: width,
                image_height: height,
                lines: Vec::new(),
            }
        }
    }
}

/// Convert OCR regions into the organizer's reading-ordered page.
fn ocr_page_from_regions(
    image_width: u32,
    image_height: u32,
    regions: Vec<DetectedTextRegion>,
) -> OcrPage {
    let mut lines: Vec<TextLine> = regions
        .into_iter()
        .filter_map(|region| {
            let text = region.text.trim();
            if text.is_empty() {
                return None;
            }
            Some(TextLine::new(
                text,
                region.bounds.x,
                region.bounds.y,
                region.bounds.width,
                region.bounds.height,
            ))
        })
        .collect();
    lines.sort_by_key(|line| (line.y, line.x));
    OcrPage {
        image_width,
        image_height,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::BoundingBox;

    fn region(text: &str, x: i32, y: i32) -> DetectedTextRegion {
        DetectedTextRegion {
            bounds: BoundingBox {
                x,
                y,
                width: 40,
                height: 12,
            },
            text: text.to_string(),
            confidence: 90,
        }
    }

    #[test]
    fn regions_become_lines_in_reading_order() {
        let page = ocr_page_from_regions(
            640,
            480,
            vec![
                region("second row", 10, 40),
                region("right", 120, 10),
                region("left", 10, 10),
            ],
        );

        let texts: Vec<&str> = page.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["left", "right", "second row"]);
        assert_eq!((page.image_width, page.image_height), (640, 480));
        assert_eq!(page.lines[0].x, 10);
        assert_eq!(page.lines[0].y, 10);
    }

    #[test]
    fn empty_regions_are_dropped() {
        let page = ocr_page_from_regions(
            320,
            200,
            vec![region("   ", 1, 1), region("", 2, 2), region("kept", 3, 3)],
        );

        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.lines[0].text, "kept");
        assert_eq!(page.image_width, 320);
        assert_eq!(page.image_height, 200);
    }

    #[test]
    fn gate_turns_off_renaming_when_the_path_already_went_out() {
        let settings = OrganizeSettings {
            auto_name: true,
            style: parse_name_style("clean"),
            excluded_apps: vec!["Signal".to_string()],
        };

        let gated = effective_settings(settings.clone(), false);
        assert!(!gated.auto_name, "a path that went out keeps its name");
        assert_eq!(gated.style, settings.style);
        assert_eq!(gated.excluded_apps, settings.excluded_apps);

        assert!(effective_settings(settings, true).auto_name);
    }
}
