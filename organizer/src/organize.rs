//! Organizes one saved capture: picks its name, renames the file, and records
//! it in the search index.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::index::{Index, IndexEntry};
use crate::naming::{suggest_name, unique_path, NameStyle};
use crate::{CaptureContext, OcrPage};

/// What the capture shell knows about one saved capture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizeRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub page: OcrPage,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub window_title: Option<String>,
    /// True for the background request that may ask the model.
    #[serde(default)]
    pub allow_ai: bool,
}

/// The user's organizer settings.
#[derive(Debug, Clone, Default)]
pub struct OrganizeSettings {
    pub auto_name: bool,
    pub style: NameStyle,
    /// App names to leave alone, e.g. "Signal" or "signal.exe".
    pub excluded_apps: Vec<String>,
}

/// What happened to the capture.
#[derive(Debug, Clone)]
pub struct OrganizeOutcome {
    /// Where the file is now (unchanged unless it was renamed).
    pub path: PathBuf,
    pub renamed: bool,
    /// False when the capture was skipped (excluded app).
    pub indexed: bool,
    /// True when the model chose the current name.
    pub ai_named: bool,
}

/// Read a stored name style; anything unrecognized is `Clean`.
pub fn parse_name_style(value: &str) -> NameStyle {
    match value.trim().to_lowercase().as_str() {
        "dated" => NameStyle::Dated,
        "by_app" => NameStyle::ByApp,
        _ => NameStyle::Clean,
    }
}

/// True when `app` matches an entry in `excluded`, ignoring case and ".exe".
pub fn is_excluded_app(app: Option<&str>, excluded: &[String]) -> bool {
    let Some(app) = app else {
        return false;
    };
    let app = normalize_app(app);
    excluded.iter().any(|entry| {
        let entry = normalize_app(entry);
        !entry.is_empty() && entry == app
    })
}

/// A trailing "@2x"-style density marker on a filename stem, or "".
fn density_suffix(stem: &str) -> &str {
    let Some(at) = stem.rfind('@') else {
        return "";
    };
    let marker = &stem[at + 1..];
    let digits = marker.strip_suffix('x').unwrap_or("");
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        &stem[at..]
    } else {
        ""
    }
}

fn normalize_app(app: &str) -> String {
    let app = app.trim().to_lowercase();
    app.strip_suffix(".exe").unwrap_or(&app).to_string()
}

/// Name, rename, and index one capture.
pub fn organize_capture(
    request: &OrganizeRequest,
    settings: &OrganizeSettings,
    index_path: &Path,
    captured_at: DateTime<Local>,
) -> OrganizeOutcome {
    organize_capture_with_ai(request, settings, index_path, captured_at, None)
}

/// Name, rename, and index one capture, optionally letting a model pick the name.
pub fn organize_capture_with_ai(
    request: &OrganizeRequest,
    settings: &OrganizeSettings,
    index_path: &Path,
    captured_at: DateTime<Local>,
    ai: Option<&dyn crate::ai::AiNamer>,
) -> OrganizeOutcome {
    if is_excluded_app(request.app.as_deref(), &settings.excluded_apps) {
        return OrganizeOutcome {
            path: request.path.clone(),
            renamed: false,
            indexed: false,
            ai_named: false,
        };
    }

    let loaded = match Index::load(index_path) {
        Ok(index) => Some(index),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            let mut aside = index_path.as_os_str().to_os_string();
            aside.push(".bad");
            let _ = fs::rename(index_path, PathBuf::from(aside));
            Index::load(index_path).ok()
        }
        Err(_) => None,
    };
    // A second request for the same capture reuses what the first one recorded.
    let previous = loaded
        .as_ref()
        .and_then(|index| index.entries().iter().find(|entry| entry.path == request.path))
        .cloned();

    let ctx = CaptureContext {
        app: request.app.clone(),
        window_title: request.window_title.clone(),
        captured_at,
    };
    let suggestion = suggest_name(&request.page, &ctx, settings.style);
    let current_stem = request
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let plain_stem = current_stem
        .strip_suffix(density_suffix(&current_stem))
        .unwrap_or(&current_stem);
    let ai_stem = match (settings.auto_name, ai) {
        (true, Some(namer)) => crate::ai::ai_name(
            namer,
            &suggestion,
            request.window_title.as_deref(),
            &request.page,
        )
        // A reply that only repeats a name we already have is not an AI name.
        .filter(|stem| {
            !stem.eq_ignore_ascii_case(&suggestion.stem)
                && !stem.eq_ignore_ascii_case(plain_stem)
        }),
        _ => None,
    };
    let chosen_stem = ai_stem.clone().unwrap_or_else(|| suggestion.stem.clone());
    let original_name = previous
        .as_ref()
        .map(|entry| entry.original_name.clone())
        .unwrap_or_else(|| {
            request
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let captured_at = previous
        .as_ref()
        .map(|entry| entry.captured_at)
        .unwrap_or(captured_at);

    let mut path = request.path.clone();
    let mut renamed = false;
    if settings.auto_name {
        if let (Some(dir), Some(ext)) = (request.path.parent(), request.path.extension()) {
            let ext = ext.to_string_lossy();
            let stem = request
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Keep a pixel-density suffix such as "@2x" the shell added.
            let density = density_suffix(&stem);
            let new_stem = format!("{}{density}", chosen_stem);
            if new_stem != stem {
                let target = unique_path(dir, &new_stem, &ext);
                if fs::rename(&request.path, &target).is_ok() {
                    path = target;
                    renamed = true;
                }
            }
        }
    }

    let (app, site, window_title, text) = if suggestion.sensitivity.withholds_text() {
        (None, None, None, String::new())
    } else {
        (
            suggestion.app.clone(),
            suggestion.site.clone(),
            request.window_title.as_deref().map(crate::secrets::scrub),
            crate::secrets::scrub(&request.page.full_text()),
        )
    };
    let ai_named = ai_stem.is_some() && renamed;
    let entry = IndexEntry {
        path: path.clone(),
        original_name,
        app,
        site,
        window_title,
        captured_at,
        text,
        auto_named: renamed,
        heuristic_name: if ai_named {
            Some(suggestion.stem.clone())
        } else {
            None
        },
        ai_named,
    };

    let mut indexed = false;
    if let Some(mut index) = loaded {
        index.rename(&request.path, &path);
        index.upsert(entry);
        indexed = index.save().is_ok();
    }

    OrganizeOutcome {
        path,
        renamed,
        indexed,
        ai_named,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextLine;
    use chrono::TimeZone;

    fn captured_at() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 27, 14, 3, 9).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "apexshot-organize-test-{}-{}-{}",
            name,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn page(lines: &[&str]) -> OcrPage {
        OcrPage {
            image_width: 1200,
            image_height: 800,
            lines: lines
                .iter()
                .enumerate()
                .map(|(index, text)| TextLine::new(*text, 0, index as i32 * 20, 600, 16))
                .collect(),
        }
    }

    fn request(path: PathBuf, window_title: Option<&str>) -> OrganizeRequest {
        OrganizeRequest {
            path,
            page: OcrPage::default(),
            app: None,
            window_title: window_title.map(str::to_string),
            allow_ai: false,
        }
    }

    struct FixedNamer(Option<String>);

    impl crate::ai::AiNamer for FixedNamer {
        fn complete(&self, _system: &str, _user: &str) -> Option<String> {
            self.0.clone()
        }
    }

    #[test]
    fn parses_name_styles() {
        assert_eq!(parse_name_style("Dated"), NameStyle::Dated);
        assert_eq!(parse_name_style(" by_app "), NameStyle::ByApp);
        assert_eq!(parse_name_style(""), NameStyle::Clean);
    }

    #[test]
    fn matches_excluded_apps_loosely() {
        assert!(is_excluded_app(Some("Signal.exe"), &["signal".to_string()]));
        assert!(!is_excluded_app(
            Some("firefox.exe"),
            &["signal".to_string()]
        ));
        assert!(!is_excluded_app(None, &["signal".to_string()]));
        assert!(!is_excluded_app(Some("x"), &["".to_string()]));
    }

    #[test]
    fn renames_from_the_window_title() {
        let dir = temp_dir("rename");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let req = request(path.clone(), Some("Quarterly report - Word"));
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };

        let outcome = organize_capture(&req, &settings, &index_path, captured_at());

        assert!(outcome.renamed);
        assert_eq!(
            outcome.path.file_name().unwrap().to_string_lossy(),
            "Quarterly report.png"
        );
        assert!(!path.exists());
        assert!(outcome.path.exists());
        assert!(outcome.indexed);

        let index = Index::load(&index_path).unwrap();
        assert_eq!(index.entries().len(), 1);
        assert_eq!(
            index.entries()[0].original_name,
            "Screenshot 2026-11-27 at 14.03.09.png"
        );
        assert_eq!(index.entries()[0].app, Some("Word".to_string()));
        assert!(index.entries()[0].auto_named);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_the_density_suffix() {
        assert_eq!(density_suffix("ApexShot-2026-11-27_18-40-50@2x"), "@2x");
        assert_eq!(density_suffix("me@example"), "");
        assert_eq!(density_suffix("plain"), "");

        let dir = temp_dir("density");
        let path = dir.join("ApexShot-2026-11-27_18-40-50@2x.png");
        fs::write(&path, b"png").unwrap();
        let req = request(path, Some("Quarterly report - Word"));
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };

        let outcome = organize_capture(&req, &settings, &dir.join("index.json"), captured_at());

        assert_eq!(
            outcome.path.file_name().unwrap().to_string_lossy(),
            "Quarterly report@2x.png"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn model_name_replaces_the_heuristic() {
        let dir = temp_dir("ai-rename");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let req = request(path, Some("Quarterly report - Word"));
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };
        let namer = FixedNamer(Some("Quarterly sales review deck".into()));

        let outcome =
            organize_capture_with_ai(&req, &settings, &index_path, captured_at(), Some(&namer));

        assert!(outcome.renamed);
        assert!(outcome.ai_named);
        assert_eq!(
            outcome.path.file_name().unwrap().to_string_lossy(),
            "Quarterly sales review deck.png"
        );

        let index = Index::load(&index_path).unwrap();
        assert_eq!(
            index.entries()[0].heuristic_name.as_deref(),
            Some("Quarterly report")
        );
        assert!(index.entries()[0].ai_named);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_when_the_model_fails() {
        let dir = temp_dir("ai-fallback");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let req = request(path, Some("Quarterly report - Word"));
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };
        let namer = FixedNamer(None);

        let outcome =
            organize_capture_with_ai(&req, &settings, &index_path, captured_at(), Some(&namer));

        assert!(outcome.renamed);
        assert!(!outcome.ai_named);
        assert_eq!(
            outcome.path.file_name().unwrap().to_string_lossy(),
            "Quarterly report.png"
        );

        let index = Index::load(&index_path).unwrap();
        assert_eq!(index.entries()[0].heuristic_name, None);
        assert!(!index.entries()[0].ai_named);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keeps_the_name_when_auto_naming_is_off() {
        let dir = temp_dir("no-rename");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let req = request(path.clone(), Some("Quarterly report - Word"));

        let outcome = organize_capture(
            &req,
            &OrganizeSettings::default(),
            &index_path,
            captured_at(),
        );

        assert!(!outcome.renamed);
        assert_eq!(outcome.path, path);
        assert!(outcome.indexed);

        let index = Index::load(&index_path).unwrap();
        assert_eq!(index.entries().len(), 1);
        assert!(!index.entries()[0].auto_named);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skips_excluded_apps_entirely() {
        let dir = temp_dir("excluded");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let mut req = request(path.clone(), Some("Quarterly report - Word"));
        req.app = Some("word.exe".to_string());
        let settings = OrganizeSettings {
            auto_name: true,
            excluded_apps: vec!["WORD".to_string()],
            ..OrganizeSettings::default()
        };

        let outcome = organize_capture(&req, &settings, &index_path, captured_at());

        assert!(!outcome.renamed);
        assert!(!outcome.indexed);
        assert!(path.exists());
        assert!(!index_path.exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn private_window_withholds_text() {
        let dir = temp_dir("private");
        let path = dir.join("shot.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let mut req = request(
            path.clone(),
            Some("Bank — Mozilla Firefox Private Browsing"),
        );
        req.page = page(&["account 12345"]);
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };

        let outcome = organize_capture(&req, &settings, &index_path, captured_at());

        let name = outcome.path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("Screenshot 2026-11-27 at "));

        let index = Index::load(&index_path).unwrap();
        assert_eq!(index.entries()[0].text, "");
        assert_eq!(index.entries()[0].window_title, None);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sets_a_corrupt_index_aside() {
        let dir = temp_dir("corrupt");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        fs::create_dir_all(index_path.parent().unwrap()).unwrap();
        fs::write(&index_path, b"not json").unwrap();
        let req = request(path.clone(), Some("Quarterly report - Word"));

        let outcome = organize_capture(
            &req,
            &OrganizeSettings::default(),
            &index_path,
            captured_at(),
        );

        assert!(outcome.indexed);
        assert!(index_path.with_file_name("index.json.bad").exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_request_keeps_the_original_name() {
        let dir = temp_dir("second-request");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let index_path = dir.join("index").join("index.json");
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };

        let first = organize_capture(
            &request(path, Some("Quarterly report - Word")),
            &settings,
            &index_path,
            captured_at(),
        );
        let mut second_request = request(first.path.clone(), Some("Quarterly report - Word"));
        second_request.allow_ai = true;
        let namer = FixedNamer(Some("Quarterly sales review deck".into()));

        let second = organize_capture_with_ai(
            &second_request,
            &settings,
            &index_path,
            captured_at(),
            Some(&namer),
        );

        assert!(second.ai_named);
        assert_eq!(
            second.path.file_name().unwrap().to_string_lossy(),
            "Quarterly sales review deck.png"
        );

        let index = Index::load(&index_path).unwrap();
        assert_eq!(index.entries().len(), 1);
        let entry = &index.entries()[0];
        assert_eq!(
            entry.original_name, "Screenshot 2026-11-27 at 14.03.09.png"
        );
        assert_eq!(entry.heuristic_name.as_deref(), Some("Quarterly report"));
        assert!(entry.ai_named);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn echoed_name_is_not_an_ai_name() {
        let dir = temp_dir("ai-echo");
        let path = dir.join("Screenshot 2026-11-27 at 14.03.09.png");
        fs::write(&path, b"png").unwrap();
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };
        let namer = FixedNamer(Some("Quarterly report".into()));

        let outcome = organize_capture_with_ai(
            &request(path, Some("Quarterly report - Word")),
            &settings,
            &dir.join("index.json"),
            captured_at(),
            Some(&namer),
        );

        assert!(!outcome.ai_named);
        assert_eq!(
            outcome.path.file_name().unwrap().to_string_lossy(),
            "Quarterly report.png"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn echoed_name_with_density_is_not_an_ai_name() {
        let dir = temp_dir("ai-echo-density");
        let path = dir.join("Quarterly report@2x.png");
        fs::write(&path, b"png").unwrap();
        let settings = OrganizeSettings {
            auto_name: true,
            ..OrganizeSettings::default()
        };
        let namer = FixedNamer(Some("quarterly REPORT".into()));

        let outcome = organize_capture_with_ai(
            &request(path.clone(), Some("Quarterly report - Word")),
            &settings,
            &dir.join("index.json"),
            captured_at(),
            Some(&namer),
        );

        assert!(!outcome.ai_named);
        assert_eq!(outcome.path, path);

        fs::remove_dir_all(&dir).unwrap();
    }
}
