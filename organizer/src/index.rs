//! The local search index: one JSON file listing every organized capture.

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};

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

impl Index {
    /// Load `path`; a missing file is an empty index.
    pub fn load(path: &Path) -> io::Result<Index> {
        Ok(Index {
            path: path.to_path_buf(),
            entries: Vec::new(),
        })
    }

    /// Write the index back to its file.
    pub fn save(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// Insert `entry`, replacing any entry with the same path.
    pub fn upsert(&mut self, entry: IndexEntry) {
        self.entries.push(entry);
    }

    /// Point the entry at `from` to `to`. False when `from` is not indexed.
    pub fn rename(&mut self, from: &Path, to: &Path) -> bool {
        let _ = (from, to);
        false
    }

    /// Drop entries whose file no longer exists; returns how many went.
    pub fn remove_missing(&mut self) -> usize {
        0
    }

    /// Entries matching `query`, best first.
    pub fn search(&self, query: &SearchQuery) -> Vec<SearchHit> {
        let _ = query;
        Vec::new()
    }
}

/// Parse what the user typed in the search box. `today` resolves relative
/// dates such as "yesterday".
pub fn parse_query(raw: &str, today: NaiveDate) -> SearchQuery {
    let _ = (raw, today);
    SearchQuery::default()
}
