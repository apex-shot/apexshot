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
    organize_capture, organize_capture_with_ai, parse_name_style, OrganizeRequest, OrganizeSettings,
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

/// The result of the rule-based pass, kept so a background AI refine can
/// reuse the OCR page and window context it already collected.
pub struct Organized {
    /// Where the capture lives after the rule-based name.
    pub path: PathBuf,
    /// The request the first pass built; None when the path went out before
    /// the worker finished collecting, or when nothing was organized.
    pub request: Option<OrganizeRequest>,
    /// True when the rule-based pass renamed the file.
    pub renamed: bool,
    /// False when the capture was skipped (an excluded app).
    pub indexed: bool,
}

/// The result of a background AI refine.
pub struct AiRefineOutcome {
    /// Where the capture lives afterwards; the rule-based path on failure.
    pub path: PathBuf,
    /// True when the model chose the current name.
    pub ai_named: bool,
}

/// Name, rename, and index a saved capture, and return where it lives now.
///
/// The original path comes back on every failure, including a missing index
/// location, an unreadable image, or a rename that lost the file.
pub fn organize_saved_capture(path: PathBuf, config: &AppConfig) -> PathBuf {
    organize_saved_capture_gated(path, config, &|| true).path
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
) -> Organized {
    let unchanged = |path: PathBuf| Organized {
        path,
        request: None,
        renamed: false,
        indexed: false,
    };
    let Some(index_path) = crate::config::organizer_index_path() else {
        return unchanged(path);
    };
    if !path.is_file() {
        return unchanged(path);
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
        return unchanged(original);
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
            return unchanged(original);
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
        return unchanged(original);
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
    Organized {
        path: outcome.path,
        request: Some(request),
        renamed: outcome.renamed,
        indexed: outcome.indexed,
    }
}

/// Refine a rule-based name with the local model, in the background.
///
/// `request` carries the OCR page and window context the first pass collected;
/// `path` is where that pass left the file. The two deadlines mirror the
/// shipped runtime: the server gets 8 s to be ready and the whole refine is
/// bounded at 12 s, so a slow machine keeps the rule-based name.
///
/// `may_rename` is the caller's gate: the Quick Access card may have used the
/// path while the rule-based name was settling, and a file the card already
/// shows must not move again. It is asked once on entry and once more after
/// the server is up, right before the model may rename anything. When it says
/// no, the capture is left exactly as the first pass indexed it.
pub fn refine_name_with_ai(
    path: PathBuf,
    mut request: OrganizeRequest,
    config: &AppConfig,
    may_rename: &dyn Fn() -> bool,
) -> AiRefineOutcome {
    let keep = |path: PathBuf| AiRefineOutcome {
        path,
        ai_named: false,
    };
    let Some(index_path) = crate::config::organizer_index_path() else {
        return keep(path);
    };
    if !may_rename() || !config.organizer_auto_name {
        // The path already went out, so there is nothing left to rename; the
        // first pass has already indexed it under that path.
        return keep(path);
    }

    request.path = path.clone();
    request.allow_ai = true;

    let started = Instant::now();
    // This runs in the background after the capture is handed out, so it can
    // wait for a model that is still loading instead of giving up.
    let naming_deadline = started + Duration::from_millis(20_000);
    let overall_deadline = started + Duration::from_millis(25_000);
    let tier = crate::ai_names::tier_from_setting(&config.organizer_ai_model);
    let Some(base_url) = crate::ai_names::ensure_server(tier, naming_deadline) else {
        return keep(path);
    };
    // Starting the server is the slow part; the card may have been used while
    // it warmed up, so ask again right before the rename is possible.
    if !may_rename() {
        return keep(path);
    }
    let settings = OrganizeSettings {
        auto_name: config.organizer_auto_name,
        style: parse_name_style(&config.organizer_name_style),
        excluded_apps: config.organizer_excluded_apps.clone(),
    };
    let namer = crate::ai_names::Namer {
        base_url,
        deadline: overall_deadline,
    };
    let outcome = organize_capture_with_ai(
        &request,
        &settings,
        &index_path,
        chrono::Local::now(),
        Some(&namer as &dyn apexshot_organizer::ai::AiNamer),
    );
    if outcome.ai_named && !crate::studio_entitlement::studio_unlocked() {
        crate::ai_names::consume_trial();
    }
    show_ai_names_card(outcome.ai_named, outcome.renamed, config, &index_path);
    if outcome.path.exists() {
        AiRefineOutcome {
            path: outcome.path,
            ai_named: outcome.ai_named,
        }
    } else {
        keep(path)
    }
}

/// Show the one AI names card this capture earned, without delaying the path.
pub(crate) fn show_ai_names_card(
    ai_named: bool,
    renamed: bool,
    config: &AppConfig,
    index_path: &Path,
) {
    use crate::ai_names::{claim_card, recommend, trial_remaining, CardKind};

    // A window that advertises something already owns the screen. Leaving the
    // card's one-off claim unspent keeps it available for the next capture.
    let slot_free = crate::promo_slot::is_free();
    let kind = if slot_free
        && ai_named
        && !crate::studio_entitlement::studio_unlocked()
        && trial_remaining() == 0
        && claim_card(CardKind::TrialEnded)
    {
        Some(CardKind::TrialEnded)
    } else if slot_free
        && !config.organizer_ai_enabled
        && recommend().tier.is_some()
        && renamed
        && index_has_auto_named(index_path, 20)
        && claim_card(CardKind::TryAiNames)
    {
        Some(CardKind::TryAiNames)
    } else {
        None
    };
    if let Some(kind) = kind {
        crate::ai_names::spawn_card(kind);
    }
}

/// True when the index holds at least `at_least` auto-named captures.
fn index_has_auto_named(path: &Path, at_least: usize) -> bool {
    let Ok(index) = apexshot_organizer::index::Index::load(path) else {
        return false;
    };
    index
        .entries()
        .iter()
        .filter(|entry| entry.auto_named)
        .count()
        >= at_least
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
    // KDE on Wayland: read the focused window through a one-shot KWin script.
    if crate::backend::kde_screenshot::is_kde_wayland_session() {
        return kde_active_window_identity().unwrap_or((None, None));
    }
    // GNOME on Wayland: a client cannot see other windows, but the ApexShot
    // Shell extension can say which one the user was in.
    gnome_extension_window_identity().unwrap_or((None, None))
}

/// The window the user was in, from the GNOME Shell extension's window list.
/// None when the extension is missing or too old to report focus.
fn gnome_extension_window_identity() -> Option<(Option<String>, Option<String>)> {
    let json = crate::utils::run_off_tokio(|| {
        let conn = zbus::blocking::connection::Builder::session()
            .ok()?
            .method_timeout(Duration::from_millis(500))
            .build()
            .ok()?;
        for name in [
            "org.apexshot.WindowList",
            "org.apexshot.ApexShot.WindowList",
        ] {
            let Ok(proxy) = zbus::blocking::Proxy::new(
                &conn,
                name,
                "/org/apexshot/WindowList",
                "org.apexshot.WindowList",
            ) else {
                continue;
            };
            if let Ok(json) = proxy.call::<_, _, String>("GetWindows", &()) {
                return Some(json);
            }
        }
        None
    })?;
    let (app, title) = pick_user_window(&json)?;
    // The extension already leaves out the capture overlay and Quick Access
    // cards, so an ApexShot window here (Settings, History) is what the user
    // captured and its title is kept.
    Some((non_empty(app), non_empty(title)))
}

/// From the extension's window list, the app and title of the window the user
/// was in: the focused one, or else the one used most recently.
fn pick_user_window(json: &str) -> Option<(Option<String>, Option<String>)> {
    let windows: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    let candidates: Vec<&serde_json::Value> = windows
        .iter()
        .filter(|window| !window["minimized"].as_bool().unwrap_or(false))
        .collect();
    let chosen = candidates
        .iter()
        .find(|window| window["focused"].as_bool().unwrap_or(false))
        .or_else(|| {
            candidates
                .iter()
                .filter(|window| window["userTime"].as_u64().unwrap_or(0) > 0)
                .max_by_key(|window| window["userTime"].as_u64().unwrap_or(0))
        })?;
    let text = |key: &str| chosen[key].as_str().map(str::to_string);
    Some((text("app"), text("title")))
}

/// How long the KDE Wayland window lookup may take before the capture
/// carries on without a title.
const KDE_WINDOW_TIMEOUT: Duration = Duration::from_millis(500);

const KWIN_SERVICE: &str = "org.kde.KWin";
const KWIN_SCRIPTING_PATH: &str = "/Scripting";
const KWIN_SCRIPTING_INTERFACE: &str = "org.kde.kwin.Scripting";
const KWIN_SCRIPT_INTERFACE: &str = "org.kde.kwin.Script";
const KDE_REPORT_PATH: &str = "/org/apexshot/WindowIdentity";
const KDE_REPORT_INTERFACE: &str = "org.apexshot.WindowIdentity";

/// The focused window's app and title on KDE Plasma with Wayland.
///
/// A client cannot ask KWin for the focused window directly, and the
/// interactive window query would need a click. Instead a short-lived KWin
/// script reads `workspace.activeWindow` and calls a temporary bus object
/// back. The lookup runs on its own thread so a slow compositor can never
/// hold a capture for longer than [`KDE_WINDOW_TIMEOUT`].
fn kde_active_window_identity() -> Option<(Option<String>, Option<String>)> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("apexshot-kde-window".to_string())
        .spawn(move || {
            let _ = tx.send(kde_active_window_report());
        })
        .ok()?;
    let report = rx.recv_timeout(KDE_WINDOW_TIMEOUT).ok().flatten()?;
    let (app, title) = parse_kde_window_report(&report)?;
    Some(clean_window_identity(app, title))
}

/// Load the reporting script into KWin and wait for its callback.
fn kde_active_window_report() -> Option<String> {
    let deadline = Instant::now() + KDE_WINDOW_TIMEOUT;
    let connection = zbus::blocking::connection::Builder::session()
        .ok()?
        .method_timeout(KDE_WINDOW_TIMEOUT)
        .build()
        .ok()?;
    let bus_name = connection.unique_name()?.to_string();
    let (tx, rx) = mpsc::channel();
    connection
        .object_server()
        .at(KDE_REPORT_PATH, KdeWindowIdentityReport { tx })
        .ok()?;

    let script_file = write_kde_window_script(&bus_name)?;
    let script_path = script_file.0.to_string_lossy().into_owned();
    let script_name = script_file
        .0
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("apexshot-window")
        .to_string();
    let loaded = connection
        .call_method(
            Some(KWIN_SERVICE),
            KWIN_SCRIPTING_PATH,
            Some(KWIN_SCRIPTING_INTERFACE),
            "loadScript",
            &(script_path.as_str(), script_name.as_str()),
        )
        .or_else(|_| {
            // Plasma 5's loadScript takes the path only.
            connection.call_method(
                Some(KWIN_SERVICE),
                KWIN_SCRIPTING_PATH,
                Some(KWIN_SCRIPTING_INTERFACE),
                "loadScript",
                &(script_path.as_str(),),
            )
        });
    let reply = loaded.ok()?;
    let script_id: i32 = reply.body().deserialize().ok()?;
    if script_id < 0 {
        return None;
    }

    let object_path = format!("/Scripting/Script{script_id}");
    let _ = connection.call_method(
        Some(KWIN_SERVICE),
        object_path.as_str(),
        Some(KWIN_SCRIPT_INTERFACE),
        "run",
        &(),
    );
    let remaining = deadline.saturating_duration_since(Instant::now());
    let report = if remaining.is_zero() {
        None
    } else {
        rx.recv_timeout(remaining).ok()
    };

    // Stop and unload by name so nothing stays registered in KWin. A failed
    // cleanup must not change the answer.
    let _ = connection.call_method(
        Some(KWIN_SERVICE),
        object_path.as_str(),
        Some(KWIN_SCRIPT_INTERFACE),
        "stop",
        &(),
    );
    let _ = connection.call_method(
        Some(KWIN_SERVICE),
        KWIN_SCRIPTING_PATH,
        Some(KWIN_SCRIPTING_INTERFACE),
        "unloadScript",
        &(script_name.as_str(),),
    );
    report
}

/// The temporary bus object the KWin script calls back.
struct KdeWindowIdentityReport {
    tx: mpsc::Sender<String>,
}

#[zbus::interface(name = "org.apexshot.WindowIdentity")]
impl KdeWindowIdentityReport {
    fn report(&self, payload: String) {
        let _ = self.tx.send(payload);
    }
}

/// A script file that removes itself when the lookup ends, including on the
/// early returns above.
struct KdeScriptFile(PathBuf);

impl Drop for KdeScriptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write the one-shot script where KWin can read it. The runtime directory is
/// shared with the host in a sandbox, unlike the private temporary directory.
fn write_kde_window_script(bus_name: &str) -> Option<KdeScriptFile> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let file_name = format!("apexshot-window-{}-{nanos}.js", std::process::id());
    let directories = [
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        Some(std::env::temp_dir()),
    ];
    for directory in directories.into_iter().flatten() {
        let path = directory.join(&file_name);
        if std::fs::write(&path, kde_window_script(bus_name)).is_ok() {
            return Some(KdeScriptFile(path));
        }
    }
    None
}

/// The script: read the focused window and report its caption and app class
/// back to ApexShot's temporary bus name.
fn kde_window_script(bus_name: &str) -> String {
    format!(
        r#"function run() {{
    var report = "";
    try {{
        var active = workspace.activeWindow;
        if (active) {{
            report = JSON.stringify({{ caption: active.caption, resourceClass: active.resourceClass }});
        }}
    }} catch (error) {{
    }}
    callDBus("{}", "{}", "{}", "Report", report);
}}
run();
"#,
        bus_name, KDE_REPORT_PATH, KDE_REPORT_INTERFACE
    )
}

#[derive(serde::Deserialize)]
struct KdeWindowReport {
    #[serde(default)]
    caption: String,
    #[serde(default, rename = "resourceClass")]
    resource_class: String,
}

/// Read the report a KWin script sent, accepting the doubly encoded string
/// some KWin versions produce.
fn parse_kde_window_report(payload: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = payload.trim();
    if payload.is_empty() {
        return None;
    }
    let report: KdeWindowReport = serde_json::from_str(payload).ok().or_else(|| {
        let unescaped: String = serde_json::from_str(payload).ok()?;
        serde_json::from_str(&unescaped).ok()
    })?;
    let app = non_empty(Some(report.resource_class));
    let title = non_empty(Some(report.caption));
    if app.is_none() && title.is_none() {
        return None;
    }
    Some((app, title))
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

/// Lowest Tesseract confidence (0-100) a line needs to be used.
const MIN_LINE_CONFIDENCE: i32 = 60;

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
            // Lines Tesseract itself doubts are noise from small or dark UI;
            // they would only reach a name or the index as gibberish.
            if text.is_empty() || region.confidence < MIN_LINE_CONFIDENCE {
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
    fn doubtful_lines_are_dropped() {
        let mut noise = region("TOM Bil QCc", 1, 1);
        noise.confidence = 31;
        let page = ocr_page_from_regions(320, 200, vec![noise, region("Settings", 1, 20)]);

        let texts: Vec<&str> = page.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["Settings"]);
    }

    #[test]
    fn the_extension_names_the_window_the_user_was_in() {
        let json = r#"[
            {"title":"Settings","app":"ApexShot","focused":false,"userTime":700,"minimized":false},
            {"title":"Pull request 283 - Firefox","app":"Firefox","focused":false,"userTime":800,"minimized":false},
            {"title":"Notes","app":"Text Editor","focused":false,"userTime":100,"minimized":false},
            {"title":"Old","app":"Files","focused":false,"userTime":950,"minimized":true}
        ]"#;
        assert_eq!(
            pick_user_window(json),
            Some((
                Some("Firefox".to_string()),
                Some("Pull request 283 - Firefox".to_string())
            ))
        );
    }

    #[test]
    fn an_old_extension_without_focus_names_nothing() {
        let json = r#"[{"title":"Firefox","app":"Firefox","minimized":false}]"#;
        assert_eq!(pick_user_window(json), None);
    }

    #[test]
    fn a_kde_report_names_the_active_window() {
        let payload = r#"{"caption":"Pull request 283 - Firefox","resourceClass":"firefox"}"#;
        assert_eq!(
            parse_kde_window_report(payload),
            Some((
                Some("firefox".to_string()),
                Some("Pull request 283 - Firefox".to_string())
            ))
        );
    }

    #[test]
    fn a_kde_report_without_a_window_names_nothing() {
        assert_eq!(parse_kde_window_report(""), None);
        assert_eq!(parse_kde_window_report("{}"), None);
    }

    #[test]
    fn an_escaped_kde_report_is_unescaped() {
        let inner = r#"{"caption":"Settings","resourceClass":"apexshot"}"#;
        let escaped = serde_json::to_string(inner).unwrap();
        assert_eq!(
            parse_kde_window_report(&escaped),
            Some((Some("apexshot".to_string()), Some("Settings".to_string())))
        );
    }

    #[test]
    fn the_kde_script_reports_the_active_window_back() {
        let script = kde_window_script(":1.42");
        assert!(script.contains("function run()"));
        assert!(script.contains("workspace.activeWindow"));
        assert!(script.contains(":1.42"));
        assert!(script.contains("resourceClass"));
        assert!(script.contains(KDE_REPORT_INTERFACE));
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

    #[test]
    fn a_used_card_keeps_the_rule_based_name() {
        let path = std::env::temp_dir().join("apexshot-ai-refine-unused.png");
        let request = OrganizeRequest {
            path: path.clone(),
            page: OcrPage::default(),
            app: None,
            window_title: None,
            allow_ai: false,
        };
        let config = AppConfig::default();

        let outcome = refine_name_with_ai(path.clone(), request, &config, &|| false);

        assert_eq!(outcome.path, path);
        assert!(!outcome.ai_named);
    }
}
