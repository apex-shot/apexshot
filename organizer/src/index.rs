//! The local search index: one JSON file listing every organized capture.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

/// Version written into every index file.
const INDEX_VERSION: u32 = 1;

/// One indexed capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Where the file lives now.
    pub path: PathBuf,
    /// The filename it was saved with, before any rename.
    pub original_name: String,
    pub app: Option<String>,
    pub site: Option<String>,
    pub window_title: Option<String>,
    pub captured_at: DateTime<Local>,
    /// Recognised text with secrets already scrubbed.
    pub text: String,
    /// True when the organizer chose the current name.
    pub auto_named: bool,
}

/// A parsed search: free text plus filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Lowercased free-text terms.
    pub terms: Vec<String>,
    pub app: Option<String>,
    pub site: Option<String>,
    /// Inclusive local-date range.
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Index into [`Index::entries`].
    pub entry: usize,
    pub score: f32,
    /// The words in the entry that matched, for highlighting.
    pub matched: Vec<String>,
}

/// The index file and its entries.
#[derive(Debug, Clone, Default)]
pub struct Index {
    path: PathBuf,
    entries: Vec<IndexEntry>,
}

/// The on-disk shape of the index.
#[derive(Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    entries: Vec<IndexEntry>,
}

impl Index {
    /// Load `path`; a missing file is an empty index.
    pub fn load(path: &Path) -> io::Result<Index> {
        match fs::read(path) {
            Ok(bytes) => {
                let file: IndexFile = serde_json::from_slice(&bytes).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid index file: {error}"),
                    )
                })?;
                Ok(Index {
                    path: path.to_path_buf(),
                    entries: file.entries,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Index {
                path: path.to_path_buf(),
                entries: Vec::new(),
            }),
            Err(error) => Err(error),
        }
    }

    /// Write the index back to its file.
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = IndexFile {
            version: INDEX_VERSION,
            entries: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &self.path)
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// Insert `entry`, replacing any entry with the same path.
    pub fn upsert(&mut self, entry: IndexEntry) {
        if let Some(index) = self.entries.iter().position(|item| item.path == entry.path) {
            self.entries[index] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Point the entry at `from` to `to`. False when `from` is not indexed.
    pub fn rename(&mut self, from: &Path, to: &Path) -> bool {
        match self.entries.iter_mut().find(|entry| entry.path == from) {
            Some(entry) => {
                entry.path = to.to_path_buf();
                true
            }
            None => false,
        }
    }

    /// Drop entries whose file no longer exists; returns how many went.
    pub fn remove_missing(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.path.exists());
        before - self.entries.len()
    }

    /// Entries matching `query`, best first.
    pub fn search(&self, query: &SearchQuery) -> Vec<SearchHit> {
        let mut hits = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if !matches_filters(entry, query) {
                continue;
            }
            if query.terms.is_empty() {
                hits.push(SearchHit {
                    entry: index,
                    score: 0.0,
                    matched: Vec::new(),
                });
                continue;
            }
            let words = entry_words(entry);
            let mut score = 0.0f32;
            let mut matched: Vec<String> = Vec::new();
            let mut all_matched = true;
            for term in &query.terms {
                match best_word(&words, term) {
                    Some((value, word)) => {
                        score += value;
                        if !matched.iter().any(|seen| seen == word) {
                            matched.push(word.to_string());
                        }
                    }
                    None => {
                        all_matched = false;
                        break;
                    }
                }
            }
            if all_matched {
                hits.push(SearchHit {
                    entry: index,
                    score,
                    matched,
                });
            }
        }
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| {
                    self.entries[right.entry]
                        .captured_at
                        .cmp(&self.entries[left.entry].captured_at)
                })
        });
        hits
    }
}

/// Parse what the user typed in the search box. `today` resolves relative
/// dates such as "yesterday".
pub fn parse_query(raw: &str, today: NaiveDate) -> SearchQuery {
    let mut query = SearchQuery::default();
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        if let Some((key, value)) = token.split_once(':') {
            if key.eq_ignore_ascii_case("app") {
                let value = value.trim();
                if !value.is_empty() {
                    query.app = Some(value.to_lowercase());
                }
                index += 1;
                continue;
            }
            if key.eq_ignore_ascii_case("site") {
                let value = value.trim().to_lowercase();
                let site = value.strip_prefix("www.").unwrap_or(value.as_str());
                if !site.is_empty() {
                    query.site = Some(site.to_string());
                }
                index += 1;
                continue;
            }
        }
        if token.eq_ignore_ascii_case("today") {
            query.from = Some(today);
            query.to = Some(today);
            index += 1;
            continue;
        }
        if token.eq_ignore_ascii_case("yesterday") {
            let date = today - Duration::days(1);
            query.from = Some(date);
            query.to = Some(date);
            index += 1;
            continue;
        }
        if token.eq_ignore_ascii_case("this") || token.eq_ignore_ascii_case("last") {
            if let Some(next) = tokens.get(index + 1) {
                let forward = token.eq_ignore_ascii_case("this");
                if next.eq_ignore_ascii_case("week") {
                    let monday = week_monday(today);
                    if forward {
                        query.from = Some(monday);
                        query.to = Some(today);
                    } else {
                        query.from = Some(monday - Duration::days(7));
                        query.to = Some(monday - Duration::days(1));
                    }
                    index += 2;
                    continue;
                }
                if next.eq_ignore_ascii_case("month") {
                    let month_start = first_of_month(today);
                    if forward {
                        query.from = Some(month_start);
                        query.to = Some(today);
                    } else {
                        let previous_end = month_start - Duration::days(1);
                        query.from = Some(first_of_month(previous_end));
                        query.to = Some(previous_end);
                    }
                    index += 2;
                    continue;
                }
            }
        }
        if let Ok(date) = NaiveDate::parse_from_str(token, "%Y-%m-%d") {
            query.from = Some(date);
            query.to = Some(date);
            index += 1;
            continue;
        }
        let lowered = token.to_lowercase();
        let term = lowered.trim_matches(|character: char| !character.is_alphanumeric());
        if !term.is_empty() {
            query.terms.push(term.to_string());
        }
        index += 1;
    }
    query
}

/// True when `entry` passes every filter in `query`.
fn matches_filters(entry: &IndexEntry, query: &SearchQuery) -> bool {
    if let Some(app) = &query.app {
        match &entry.app {
            Some(value) if value.to_lowercase().contains(app) => {}
            _ => return false,
        }
    }
    if let Some(site) = &query.site {
        match &entry.site {
            Some(value) => {
                let value = value.to_lowercase();
                let dotted = format!(".{site}");
                if value.as_str() != site.as_str() && !value.ends_with(&dotted) {
                    return false;
                }
            }
            None => return false,
        }
    }
    let day = entry.captured_at.date_naive();
    if let Some(from) = query.from {
        if day < from {
            return false;
        }
    }
    if let Some(to) = query.to {
        if day > to {
            return false;
        }
    }
    true
}

/// Every distinct lowercased word of `entry`, in source order.
fn entry_words(entry: &IndexEntry) -> Vec<String> {
    let mut words = Vec::new();
    let mut seen = HashSet::new();
    collect_words(&entry.text, &mut words, &mut seen);
    if let Some(title) = &entry.window_title {
        collect_words(title, &mut words, &mut seen);
    }
    if let Some(app) = &entry.app {
        collect_words(app, &mut words, &mut seen);
    }
    if let Some(site) = &entry.site {
        collect_words(site, &mut words, &mut seen);
    }
    if let Some(stem) = entry.path.file_stem() {
        let stem = stem.to_string_lossy();
        collect_words(stem.as_ref(), &mut words, &mut seen);
    }
    words
}

/// Append the distinct lowercased words of `text` to `words`.
fn collect_words(text: &str, words: &mut Vec<String>, seen: &mut HashSet<String>) {
    for word in text.unicode_words() {
        let word = word.to_lowercase();
        if !word.is_empty() && seen.insert(word.clone()) {
            words.push(word);
        }
    }
}

/// The highest score any word reaches for `term`, earliest word on ties.
fn best_word<'a>(words: &'a [String], term: &str) -> Option<(f32, &'a str)> {
    let mut best: Option<(f32, &'a str)> = None;
    for word in words {
        if let Some(score) = word_score(word, term) {
            let better = match best {
                Some((best_score, _)) => score > best_score,
                None => true,
            };
            if better {
                best = Some((score, word.as_str()));
            }
        }
    }
    best
}

/// Score one term against one word, or `None` when they do not match.
fn word_score(word: &str, term: &str) -> Option<f32> {
    if word == term {
        return Some(3.0);
    }
    let term_chars = term.chars().count();
    if term_chars >= 2 && word.starts_with(term) {
        return Some(2.0);
    }
    if term_chars >= 3 && word.contains(term) {
        return Some(1.5);
    }
    let allowed = if (4..=7).contains(&term_chars) {
        1
    } else if term_chars >= 8 {
        2
    } else {
        return None;
    };
    let word_chars = word.chars().count();
    if word_chars > term_chars + allowed || term_chars > word_chars + allowed {
        return None;
    }
    if osa_distance(word, term) <= allowed {
        Some(1.0)
    } else {
        None
    }
}

/// Optimal string alignment distance: character edits plus adjacent swaps.
fn osa_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let rows = left.len();
    let columns = right.len();
    let mut table = vec![vec![0usize; columns + 1]; rows + 1];
    for (row, cells) in table.iter_mut().enumerate() {
        cells[0] = row;
    }
    for (column, cell) in table[0].iter_mut().enumerate() {
        *cell = column;
    }
    for row in 1..=rows {
        for column in 1..=columns {
            let cost = if left[row - 1] == right[column - 1] {
                0
            } else {
                1
            };
            let mut distance = (table[row - 1][column] + 1)
                .min(table[row][column - 1] + 1)
                .min(table[row - 1][column - 1] + cost);
            if row > 1
                && column > 1
                && left[row - 1] == right[column - 2]
                && left[row - 2] == right[column - 1]
            {
                distance = distance.min(table[row - 2][column - 2] + 1);
            }
            table[row][column] = distance;
        }
    }
    table[rows][columns]
}

/// The Monday of the ISO week containing `date`.
fn week_monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// The first day of the month containing `date`.
fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;

    fn entry(path: &str, text: &str, app: Option<&str>, day: u32) -> IndexEntry {
        IndexEntry {
            path: PathBuf::from(path),
            original_name: path.to_string(),
            app: app.map(|value| value.to_string()),
            site: None,
            window_title: None,
            captured_at: Local.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap(),
            text: text.to_string(),
            auto_named: false,
        }
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn index(entries: Vec<IndexEntry>) -> Index {
        Index {
            path: PathBuf::from("index.json"),
            entries,
        }
    }

    fn temp_dir(name: String) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_app_and_relative_date() {
        let query = parse_query("invoice app:Firefox yesterday", date(2026, 11, 27));
        assert_eq!(query.terms, vec!["invoice".to_string()]);
        assert_eq!(query.app.as_deref(), Some("firefox"));
        assert_eq!(query.from, Some(date(2026, 11, 26)));
        assert_eq!(query.to, Some(date(2026, 11, 26)));
    }

    #[test]
    fn parses_this_week() {
        let query = parse_query("this week", date(2026, 11, 27));
        assert!(query.terms.is_empty());
        assert_eq!(query.from, Some(date(2026, 11, 23)));
        assert_eq!(query.to, Some(date(2026, 11, 27)));
    }

    #[test]
    fn parses_last_month() {
        let query = parse_query("last month", date(2026, 11, 27));
        assert!(query.terms.is_empty());
        assert_eq!(query.from, Some(date(2026, 10, 1)));
        assert_eq!(query.to, Some(date(2026, 10, 31)));
    }

    #[test]
    fn parses_site_and_quoted_term() {
        let query = parse_query("site:www.GitHub.com \"login\"", date(2026, 11, 27));
        assert_eq!(query.site.as_deref(), Some("github.com"));
        assert_eq!(query.terms, vec!["login".to_string()]);
    }

    #[test]
    fn exact_word_outranks_prefix() {
        let index = index(vec![
            entry("a.png", "login page", None, 20),
            entry("b.png", "loginbox", None, 21),
        ]);
        let hits = index.search(&parse_query("login", date(2026, 11, 27)));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry, 0);
        assert_eq!(hits[0].score, 3.0);
        assert_eq!(hits[1].entry, 1);
    }

    #[test]
    fn one_typo_still_matches() {
        let index = index(vec![entry("a.png", "authentication failed", None, 20)]);
        let query = SearchQuery {
            terms: vec!["authentcation".to_string()],
            ..SearchQuery::default()
        };
        let hits = index.search(&query);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].matched, vec!["authentication".to_string()]);
    }

    #[test]
    fn all_terms_must_match() {
        let index = index(vec![entry("a.png", "login page", None, 20)]);
        let query = SearchQuery {
            terms: vec!["login".to_string(), "zebra".to_string()],
            ..SearchQuery::default()
        };
        assert!(index.search(&query).is_empty());
    }

    #[test]
    fn filename_stem_is_searchable() {
        let index = index(vec![entry("/shots/quarterly-report.png", "", None, 20)]);
        let query = SearchQuery {
            terms: vec!["quarterly".to_string()],
            ..SearchQuery::default()
        };
        let hits = index.search(&query);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].matched, vec!["quarterly".to_string()]);
    }

    #[test]
    fn date_filter_excludes_other_days() {
        let index = index(vec![
            entry("old.png", "login", None, 20),
            entry("new.png", "login", None, 26),
        ]);
        let query = SearchQuery {
            terms: vec!["login".to_string()],
            from: Some(date(2026, 11, 26)),
            to: Some(date(2026, 11, 26)),
            ..SearchQuery::default()
        };
        let hits = index.search(&query);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry, 1);
    }

    #[test]
    fn upsert_replaces_and_rename_updates() {
        let mut index = Index::default();
        index.upsert(entry("/shots/a.png", "first", None, 20));
        index.upsert(entry("/shots/b.png", "second", None, 21));
        index.upsert(entry("/shots/a.png", "third", None, 22));
        assert_eq!(index.entries().len(), 2);
        assert_eq!(index.entries()[0].text, "third");
        assert!(index.rename(Path::new("/shots/b.png"), Path::new("/shots/c.png")));
        assert_eq!(index.entries()[1].path, PathBuf::from("/shots/c.png"));
        assert!(!index.rename(Path::new("/shots/missing.png"), Path::new("/shots/d.png")));
    }

    #[test]
    fn saves_and_loads_round_trip() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = temp_dir(format!(
            "apexshot-organizer-index-test-{}-{}",
            std::process::id(),
            nanos
        ));
        let path = dir.join("index.json");

        let mut index = Index::load(&path).unwrap();
        assert!(index.entries().is_empty());
        index.upsert(entry("/shots/a.png", "hello", Some("Firefox"), 20));
        index.save().unwrap();

        let loaded = Index::load(&path).unwrap();
        assert_eq!(loaded.entries(), index.entries());

        let missing = Index::load(&dir.join("missing.json")).unwrap();
        assert!(missing.entries().is_empty());

        let bad = dir.join("bad.json");
        fs::write(&bad, b"not json").unwrap();
        let error = Index::load(&bad).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_missing_drops_gone_files() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let dir = temp_dir(format!(
            "apexshot-organizer-index-test-{}-{}-missing",
            std::process::id(),
            nanos
        ));
        let kept = dir.join("keep.png");
        fs::write(&kept, b"x").unwrap();
        let kept_text = kept.to_string_lossy().into_owned();
        let gone_text = dir.join("gone.png").to_string_lossy().into_owned();

        let mut index = Index::default();
        index.upsert(entry(&kept_text, "kept", None, 20));
        index.upsert(entry(&gone_text, "gone", None, 21));

        assert_eq!(index.remove_missing(), 1);
        assert_eq!(index.entries().len(), 1);
        assert_eq!(index.entries()[0].text, "kept");

        fs::remove_dir_all(&dir).unwrap();
    }
}
